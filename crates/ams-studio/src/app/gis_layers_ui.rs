//! The layers sidebar of the GIS pane, like the layer list of QGIS: the built-in layers (basemap and specimen symbols,
//! pinned at the top above a thin separator, only a switch) and below it the map and vector layers you add, in groups. Tick boxes switch layers and groups on and off, a drag moves a layer or a group (before, after or
//! into another row), a double click renames, a right click has the rest, and under the list are the properties of the
//! selected layer (opacity, georeference, zoom, remove). The list itself is `gis_layers.rs`.

use super::gis_georef::crs_picker;
use super::gis_layers::{Drop, Kind};
use super::gis_ui::{pick_file, View};
use super::{info_icon, App, W_GIS};
use crate::plots::Theme;
use ams_core::gis::Georef;
use egui::{Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

const ROW_H: f32 = 28.0;

/// What the sidebar asked for during a frame; applied when the lists are no longer borrowed.
/// The opacity of a layer as it is set in the right-click menu: a slider, the value as a percentage.
fn opacity_slider(ui: &mut egui::Ui, value: &mut f32) {
    ui.label(RichText::new("Opacity").small());
    ui.set_min_width(220.0);
    super::thin_slider(ui, egui::Slider::new(value, 0.05..=1.0).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)).custom_parser(|t| t.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|p| p / 100.0)));
}

enum Act {
    Select(u32),
    /// A click with Shift (range) or Ctrl (toggle) held.
    SelectMods(u32, bool, bool),
    SelectAll,
    RemoveMany(Vec<u32>),
    Undo,
    ToggleVisible(u32),
    ToggleOpen(u32),
    Rename(u32),
    Georeference(u32),
    Zoom(u32),
    Remove(u32),
    AddMap,
    ImportQgis,
    AddGroup,
    Move(u32, u32, Drop),
    UseSystem(u32),
    SymbolSettings,
    PopOut,
    RemoveAll(u32),
    UseVecSystem(u32),
    /// Select the layer and open its filter box.
    FocusFilter(u32),
    SetLabel(u32, String),
    MoveTop(u32),
    MoveBottom(u32),
    Duplicate(u32),
}

/// Where a drop at `p` lands: the row it is over and before, after or into it. The upper quarter of a row is before it,
/// the lower quarter after it, the middle is into it when it is a group (and before or after by halves when not). Below
/// the last row it is after it, above the first row before it.
pub fn drop_for(rows: &[(u32, Rect)], p: Pos2, is_group: &dyn Fn(u32) -> bool) -> Option<(u32, Drop)> {
    let (first, last) = (rows.first()?, rows.last()?);
    if p.y < first.1.min.y {
        return Some((first.0, Drop::Before));
    }
    if p.y > last.1.max.y {
        return Some((last.0, Drop::After));
    }
    let (id, r) = rows.iter().find(|(_, r)| p.y >= r.min.y && p.y <= r.max.y)?;
    let f = (p.y - r.min.y) / r.height().max(1.0);
    Some((
        *id,
        if f < 0.25 {
            Drop::Before
        } else if f > 0.75 {
            Drop::After
        } else if is_group(*id) {
            Drop::Inside
        } else if f < 0.5 {
            Drop::Before
        } else {
            Drop::After
        },
    ))
}

fn paint_check(ui: &Ui, r: Rect, on: bool, dim: bool, accent: Color32) {
    let stroke = ui.visuals().widgets.inactive.fg_stroke.color;
    let col = if dim { stroke.gamma_multiply(0.45) } else { stroke };
    ui.painter().rect(r, 3.0, if on { accent.gamma_multiply(if dim { 0.35 } else { 0.9 }) } else { ui.visuals().extreme_bg_color }, Stroke::new(1.2, col), StrokeKind::Inside);
    if on {
        let c = r.center();
        let w = Stroke::new(1.8, Color32::WHITE);
        ui.painter().line_segment([c + Vec2::new(-3.5, 0.5), c + Vec2::new(-1.0, 3.0)], w);
        ui.painter().line_segment([c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.0, -3.0)], w);
    }
}

impl App {
    /// The layers sidebar, drawn in `rect` at the left of the GIS pane.
    pub(super) fn gis_layer_bar(&mut self, ui: &mut Ui, rect: Rect, th: &Theme) {
        let dark = self.dark;
        ui.painter().rect(rect, 6.0, if dark { Color32::from_gray(33) } else { Color32::from_gray(247) }, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink(8.0)).layout(Layout::top_down(Align::Min)));
        // nothing of the sidebar may spill out of its rectangle, however short the pane is
        child.set_clip_rect(rect.shrink(2.0).intersect(ui.clip_rect()));
        let ui = &mut child;
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 7.0);
        let ctx = ui.ctx().clone();
        let mut acts: Vec<Act> = vec![];
        // the whole sidebar scrolls as one: the header, the rows and the properties of the selected layer
        egui::ScrollArea::vertical().id_salt("gis-layer-bar").auto_shrink([false, false]).show(ui, |ui| {
        // the very first thing: the viewer into a window of its own (and back)
        let popped = self.gis_popped;
        if ui.add(egui::Button::new(if popped { "Dock back into the main window" } else { "Pop out into its own window" }).min_size(Vec2::new(ui.available_width(), 26.0))).on_hover_text("Makes the GIS viewer a window of its own that can be moved to another screen, or puts it back into the panes. Shift+Ctrl+Space does the same (the chord tiling window managers use to float a window)").clicked() {
            acts.push(Act::PopOut);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layers").strong());
            info_icon(ui, "Like the layer list of QGIS. The tick box switches a layer or a whole group on and off; drag a layer or a group to move it (onto the upper or lower edge of a row to put it before or after, onto the middle of a group to put it inside); double-click renames; right-click has the rest. What is higher in the list is drawn above what is lower. Shift+Ctrl+B shows and hides this sidebar.\nSeveral layers at once: Shift+drag a rectangle over the rows, Shift+click for a range, Ctrl+click for single ones, Ctrl+A for all; Delete removes the selected layers and Ctrl+Z puts them back. Removing only takes a layer out of this list; no map file and no QGIS or ArcGIS project is ever changed.");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Add layer ...").on_hover_text("A map image or a GeoTIFF, or vector data: a shapefile (.shp, with its .prj), a GeoPackage (.gpkg, every feature table becomes a layer) or a GeoJSON file").clicked() {
                    acts.push(Act::AddMap);
                }
            if ui.small_button("QGIS / ArcGIS project ...").on_hover_text("Adds the layers of a QGIS project (.qgz, .qgs) or an ArcGIS project (ArcGIS Pro: .aprx, .lyrx, .mapx; ArcMap: .mxd, .lyr) with its groups, order, tick boxes, names, transparency, filters (definition queries) and label fields. You can also drop the file onto the window. ArcMap files keep no readable ticks, groups or filters, so only their layers are found. Only the paths in the project are used: the files are read when a layer is switched on, styles are not carried over, online layers (the satellite basemap is built in) and missing files are left out and listed here").clicked() {
                    acts.push(Act::ImportQgis);
                }
            if ui.small_button("Add group").clicked() {
                    acts.push(Act::AddGroup);
                }
        });
        if !self.gis.map_err.is_empty() {
            ui.label(RichText::new(&self.gis.map_err).small().color(Color32::from_rgb(200, 80, 40)));
        }
        if !self.gis.info.is_empty() {
            ui.label(RichText::new(&self.gis.info).small().weak());
        }

        // ---- the rows: the built-in layers first (pinned, only a switch), a thin separator, then the added layers
        let rows = self.gis.layers.rows();
        let mut row_rects: Vec<(u32, Rect)> = vec![];
        let all: Vec<(usize, usize, bool)> = self.gis.layers.builtin_rows().into_iter().map(|i| (i, 0, true)).chain(rows.iter().map(|&(i, d)| (i, d, false))).collect();
        let mods = ui.input(|i| i.modifiers);
        let multi_menu: Option<Vec<u32>> = (self.gis.multi.len() > 1).then(|| self.gis.multi.clone());
        let undo_label: Option<String> = self.gis.layer_undo.last().map(|(w, _)| w.clone());
        let mut separated = false;
        let separator = |ui: &mut Ui| {
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
            ui.add_space(2.0);
        };
        for (idx, depth, is_builtin) in all {
            if !is_builtin && !separated {
                separated = true;
                separator(ui);
            }
            {
                let it = self.gis.layers.items[idx].clone();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
                if !is_builtin {
                    row_rects.push((it.id, rect));
                }
                let selected = self.gis.sel == Some(it.id) || self.gis.multi.contains(&it.id);
                if selected {
                    ui.painter().rect_filled(rect, 4.0, th.accent.gamma_multiply(0.22));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.5));
                }
                let x0 = rect.min.x + 4.0 + depth as f32 * 16.0;
                let arrow = Rect::from_min_size(Pos2::new(x0, rect.center().y - 9.0), Vec2::splat(18.0));
                let cb = Rect::from_min_size(Pos2::new(x0 + 20.0, rect.center().y - 7.0), Vec2::splat(14.0));
                if it.kind == Kind::Group {
                    ui.painter().text(arrow.center(), egui::Align2::CENTER_CENTER, if it.open { "▾" } else { "▸" }, egui::FontId::proportional(13.0), ui.visuals().text_color());
                }
                let own_on = if it.kind == Kind::Basemap { self.gis.base_on } else { it.visible };
                let shown = self.gis.layers.effective_visible(it.id) || (it.kind == Kind::Basemap && own_on);
                paint_check(ui, cb, own_on, !shown && own_on, th.accent);
                let name_x = cb.max.x + 8.0;
                let grip_w = if it.kind == Kind::Group { 16.0 } else { 0.0 };
                let name_rect = Rect::from_min_max(Pos2::new(name_x, rect.min.y + 2.0), Pos2::new(rect.max.x - 4.0 - grip_w, rect.max.y - 2.0));
                // a group has a small grip at its right end: dragging it only reorders the group among its siblings, it can
                // never drop it into another group by accident
                if it.kind == Kind::Group {
                    let gr = Rect::from_center_size(Pos2::new(rect.max.x - 11.0, rect.center().y), Vec2::new(14.0, ROW_H - 4.0));
                    let gresp = ui.interact(gr, ui.scope_id().with(("gis-group-grip", it.id)), Sense::drag());
                    let col = if gresp.hovered() || gresp.dragged() { th.accent } else { ui.visuals().weak_text_color() };
                    for dy in [-4.0f32, 0.0, 4.0] {
                        for dx in [-2.0f32, 2.0] {
                            ui.painter().circle_filled(gr.center() + Vec2::new(dx, dy), 1.1, col);
                        }
                    }
                    if gresp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        gresp.clone().on_hover_text("Drag to move this group among the others (it never goes into a group)");
                    }
                    if gresp.drag_started() {
                        self.gis.layer_drag = Some(it.id);
                        self.gis.drag_reorder = true;
                    }
                }
                let renaming = self.gis.renaming.as_ref().filter(|(id, _)| *id == it.id).is_some();
                if renaming {
                    if let Some((_, buf)) = &mut self.gis.renaming {
                        let te = ui.put(name_rect, egui::TextEdit::singleline(buf));
                        te.request_focus();
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let clicked_away = ui.input(|i| i.pointer.any_pressed()) && !te.hovered();
                        if enter || clicked_away {
                            let name = buf.trim().to_string();
                            if !name.is_empty() {
                                if let Some(item) = self.gis.layers.get_mut(it.id) {
                                    item.name = name;
                                }
                            }
                            self.gis.renaming = None;
                        } else if esc {
                            self.gis.renaming = None;
                        }
                    }
                } else {
                    let col = if shown || it.kind == Kind::Group { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
                    let label = if it.kind == Kind::Group { RichText::new(&it.name).strong() } else { RichText::new(&it.name) };
                    // a name that does not fit ends in an ellipsis (the whole name is in the tooltip)
                    let mut job = egui::text::LayoutJob::simple_singleline(label.text().to_string(), egui::FontId::proportional(13.5), col);
                    job.wrap = egui::text::TextWrapping { max_width: name_rect.width().max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
                    let g = ui.painter().layout_job(job);
                    let cut = g.rows.first().is_some_and(|r| r.ends_with_newline) || g.text().ends_with('\u{2026}') && !it.name.ends_with('\u{2026}');
                    let clip = ui.painter().with_clip_rect(name_rect);
                    clip.galley(Pos2::new(name_x, rect.center().y - g.size().y / 2.0), g, col);
                    if cut {
                        resp.clone().on_hover_text(&it.name);
                    }
                }
                let pos = resp.interact_pointer_pos().unwrap_or(rect.center());
                if resp.clicked() {
                    if it.kind == Kind::Group && arrow.expand(2.0).contains(pos) {
                        acts.push(Act::ToggleOpen(it.id));
                    } else if cb.expand(3.0).contains(pos) {
                        acts.push(Act::ToggleVisible(it.id));
                    } else if mods.shift || mods.command {
                        acts.push(Act::SelectMods(it.id, mods.shift, mods.command));
                    } else {
                        acts.push(Act::Select(it.id));
                    }
                }
                // a right click selects the row too (unless it is one of several selected), and its menu has the delete
                if resp.secondary_clicked() && !self.gis.multi.contains(&it.id) {
                    acts.push(Act::Select(it.id));
                }
                if resp.double_clicked() && !is_builtin && !cb.expand(3.0).contains(pos) {
                    acts.push(Act::Rename(it.id));
                }
                // Shift+drag over the rows selects every row the rectangle touches; a drag without Shift moves the layer
                if resp.drag_started() && !is_builtin && mods.shift && !renaming && self.gis.layer_drag.is_none() {
                    self.gis.band = Some(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
                    if !mods.command {
                        self.gis.multi.clear();
                    }
                } else if resp.drag_started() && !is_builtin && !cb.expand(3.0).contains(pos) && !renaming && !self.gis.drag_reorder && self.gis.layer_drag.is_none() && self.gis.band.is_none() {
                    self.gis.layer_drag = Some(it.id);
                }
                resp.context_menu(|ui| {
                    if let Some(m) = &multi_menu {
                        if ui.button(RichText::new(format!("Delete the selected layers (n={})", m.len())).color(Color32::from_rgb(210, 70, 60))).on_hover_text("Takes them out of the list; no file is touched. Ctrl+Z puts them back").clicked() {
                            acts.push(Act::RemoveMany(m.clone()));
                            ui.close();
                        }
                        ui.separator();
                    }
                    if let Some(w) = &undo_label {
                        if ui.button(format!("Undo: {w}")).on_hover_text("Ctrl+Z over this list").clicked() {
                            acts.push(Act::Undo);
                            ui.close();
                        }
                        ui.separator();
                    }
                    match it.kind {
                        Kind::Map => {
                            if ui.button("Rename").clicked() {
                                acts.push(Act::Rename(it.id));
                                ui.close();
                            }
                            if ui.button("Georeference ...").clicked() {
                                acts.push(Act::Georeference(it.id));
                                ui.close();
                            }
                            if ui.button("Zoom to layer").clicked() {
                                acts.push(Act::Zoom(it.id));
                                ui.close();
                            }
                            ui.separator();
                            if let Some(m) = self.gis.maps.iter_mut().find(|m| m.id == it.id) {
                                opacity_slider(ui, &mut m.opacity);
                                ui.separator();
                            }
                            if ui.button("Move to top").clicked() {
                                acts.push(Act::MoveTop(it.id));
                                ui.close();
                            }
                            if ui.button("Move to bottom").clicked() {
                                acts.push(Act::MoveBottom(it.id));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(RichText::new("Delete layer").color(Color32::from_rgb(210, 70, 60))).on_hover_text("Takes the layer out of the list; the map file is not touched").clicked() {
                                acts.push(Act::Remove(it.id));
                                ui.close();
                            }
                        }
                        Kind::Vector => {
                            if ui.button("Rename").clicked() {
                                acts.push(Act::Rename(it.id));
                                ui.close();
                            }
                            if ui.button("Zoom to layer").clicked() {
                                acts.push(Act::Zoom(it.id));
                                ui.close();
                            }
                            ui.separator();
                            if let Some(v) = self.gis.vectors.iter_mut().find(|v| v.id == it.id) {
                                if !v.data.fields.is_empty() {
                                    if ui.button("Filter ...").on_hover_text("Show only some features, with a condition written like the QGIS Filter dialog").clicked() {
                                        acts.push(Act::FocusFilter(it.id));
                                        ui.close();
                                    }
                                    ui.menu_button("Labels", |ui| {
                                        if ui.selectable_label(v.label.is_empty(), "(none)").clicked() {
                                            acts.push(Act::SetLabel(it.id, String::new()));
                                            ui.close();
                                        }
                                        for f in &v.data.fields {
                                            if ui.selectable_label(&v.label == f, f).clicked() {
                                                acts.push(Act::SetLabel(it.id, f.clone()));
                                                ui.close();
                                            }
                                        }
                                    });
                                }
                                if ui.button("Duplicate layer").on_hover_text("A copy with the same filter and look, for another filter of the same file").clicked() {
                                    acts.push(Act::Duplicate(it.id));
                                    ui.close();
                                }
                                ui.separator();
                                opacity_slider(ui, &mut v.opacity);
                                ui.separator();
                            }
                            if ui.button("Move to top").clicked() {
                                acts.push(Act::MoveTop(it.id));
                                ui.close();
                            }
                            if ui.button("Move to bottom").clicked() {
                                acts.push(Act::MoveBottom(it.id));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(RichText::new("Delete layer").color(Color32::from_rgb(210, 70, 60))).on_hover_text("Takes the layer out of the list; the file is not touched").clicked() {
                                acts.push(Act::Remove(it.id));
                                ui.close();
                            }
                        }
                        Kind::Group => {
                            if ui.button("Rename").clicked() {
                                acts.push(Act::Rename(it.id));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(RichText::new("Delete group (keep its layers)").color(Color32::from_rgb(210, 70, 60))).on_hover_text("The layers of the group move up into its place").clicked() {
                                acts.push(Act::Remove(it.id));
                                ui.close();
                            }
                            if ui.button(RichText::new("Delete group and its layers").color(Color32::from_rgb(210, 70, 60))).clicked() {
                                acts.push(Act::RemoveAll(it.id));
                                ui.close();
                            }
                        }
                        Kind::Basemap | Kind::Specimens => {
                            if ui.button(if own_on { "Hide" } else { "Show" }).clicked() {
                                acts.push(Act::ToggleVisible(it.id));
                                ui.close();
                            }
                            ui.separator();
                            ui.label(RichText::new("Built in: it can only be switched on and off").small().weak());
                        }
                    }
                });
            }
        }
        if !separated {
            separator(ui);
            ui.label(RichText::new("No layers added yet: Add layer ... above.").small().weak());
        }
        // keys while the pointer is over the sidebar and nothing is being typed: Delete removes the selected layers, Ctrl+A
        // selects every layer, Ctrl+Z puts removed layers back, Escape drops the selection
        let over_bar = ui.rect_contains_pointer(rect) && self.gis.renaming.is_none() && !ui.ctx().egui_wants_keyboard_input();
        if over_bar {
            if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                if self.gis.multi.len() > 1 {
                    acts.push(Act::RemoveMany(self.gis.multi.clone()));
                } else if let Some(id) = self.gis.sel.filter(|id| self.gis.layers.get(*id).map_or(false, |i| !i.kind.is_builtin())) {
                    acts.push(Act::Remove(id));
                }
            }
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
                acts.push(Act::SelectAll);
            }
            if ui.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z)) {
                acts.push(Act::Undo);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !self.gis.multi.is_empty() {
                self.gis.multi.clear();
            }
        }
        // the rectangle of a Shift+drag: the rows it touches are the selection
        if let Some(start) = self.gis.band {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                    let band = Rect::from_two_pos(start, p);
                    let keep: Vec<u32> = if mods.command { self.gis.multi.clone() } else { vec![] };
                    let mut hit: Vec<u32> = keep;
                    for (id, r) in &row_rects {
                        if r.intersects(band) && !hit.contains(id) {
                            hit.push(*id);
                        }
                    }
                    self.gis.multi = hit;
                    ui.painter().rect(band, 2.0, th.accent.gamma_multiply(0.18), Stroke::new(1.2, th.accent), StrokeKind::Inside);
                    ui.ctx().request_repaint();
                }
            } else {
                self.gis.band = None;
                if let Some(first) = self.gis.multi.first().copied() {
                    self.gis.sel = Some(first);
                    self.gis.multi_anchor = Some(first);
                }
                if self.gis.multi.len() == 1 {
                    self.gis.multi.clear();
                }
            }
        }
        self.gis.row_hits = row_rects.clone();
        if self.gis.multi.len() > 1 {
            ui.label(RichText::new(format!("Selected layers (n={}): Delete removes them, Ctrl+Z puts them back", self.gis.multi.len())).small().color(th.accent));
        }

        // ---- a drag in progress: where it would land, and the drop
        if let Some(dragged) = self.gis.layer_drag {
            let pointer = ui.input(|i| i.pointer.latest_pos());
            let reorder = self.gis.drag_reorder;
            let is_group = |id: u32| !reorder && self.gis.layers.get(id).map_or(false, |i| i.kind == Kind::Group);
            // dragged by its grip, a group only lands before or after one of the groups and layers at its own level: a row
            // inside a sibling stands for the whole sibling
            let drop_rows: Vec<(u32, Rect)> = if reorder {
                let t = &self.gis.layers;
                let parent = t.get(dragged).and_then(|i| i.parent);
                t.items
                    .iter()
                    .filter(|i| i.parent == parent && !i.kind.is_builtin())
                    .filter_map(|sib| {
                        let mut it = row_rects.iter().filter(|(id, _)| *id == sib.id || t.is_inside(*id, sib.id)).map(|(_, r)| *r);
                        let first = it.next()?;
                        Some((sib.id, it.fold(first, |a, b| a.union(b))))
                    })
                    .collect()
            } else {
                row_rects.clone()
            };
            let target = pointer.filter(|p| p.x >= rect.min.x && p.x <= rect.max.x).and_then(|p| drop_for(&drop_rows, p, &is_group));
            if !ui.input(|i| i.pointer.primary_down()) {
                if let Some((t, d)) = target {
                    acts.push(Act::Move(dragged, t, d));
                }
                self.gis.layer_drag = None;
                self.gis.drag_reorder = false;
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                ui.ctx().request_repaint();
                if let Some((t, d)) = target {
                    if let Some((_, r)) = drop_rows.iter().find(|(id, _)| *id == t) {
                        let line = Stroke::new(2.5, th.accent);
                        match d {
                            Drop::Before => {
                                ui.painter().line_segment([r.left_top(), r.right_top()], line);
                            }
                            Drop::After => {
                                ui.painter().line_segment([r.left_bottom(), r.right_bottom()], line);
                            }
                            Drop::Inside => {
                                ui.painter().rect_stroke(r.shrink(1.0), 4.0, line, StrokeKind::Inside);
                            }
                        }
                    }
                }
                if let (Some(p), Some(item)) = (pointer, self.gis.layers.get(dragged)) {
                    let g = ui.painter().layout_no_wrap(item.name.clone(), egui::FontId::proportional(13.0), Color32::WHITE);
                    let r = Rect::from_min_size(p + Vec2::new(12.0, 10.0), g.size() + Vec2::new(14.0, 8.0));
                    ui.painter().rect_filled(r, 5.0, th.accent);
                    ui.painter().galley(r.min + Vec2::new(7.0, 4.0), g, Color32::WHITE);
                }
            }
        }

        // ---- the properties of the selected layer
        ui.separator();
        let g = &mut self.gis;
        let filter_focused = g.filter_focus.take();
        g.filter_focus = filter_focused;
        let sel = g.sel.and_then(|id| g.layers.get(id).cloned());
        match sel {
            None => {
                ui.label(RichText::new("Click a layer to select it. Drag layers to reorder them or onto a group; right-click for more.").small().weak());
            }
            Some(item) if item.kind == Kind::Map => {
                ui.label(RichText::new(&item.name).strong());
                if let Some(m) = g.maps.iter_mut().find(|m| m.id == item.id) {
                    ui.label(RichText::new(m.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).small().weak());
                    ui.label(RichText::new(&m.note).small().weak());
                    if m.pending.is_some() {
                        ui.label(RichText::new("Which coordinate system are the coordinates of the map file in?").strong().small());
                        crs_picker(ui, "gis-pending", &mut g.pending_crs);
                        if ui.button("Use this system").clicked() {
                            acts.push(Act::UseSystem(item.id));
                        }
                    } else if m.georef.is_none() {
                        ui.label(RichText::new("Not placed yet: use Georeference.").small().color(Color32::from_rgb(200, 120, 30)));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Georeference ...").on_hover_text("Place the map by clicking matching points on the map and on the basemap").clicked() {
                            acts.push(Act::Georeference(item.id));
                        }
                        if ui.add_enabled(m.georef.is_some(), egui::Button::new("Zoom to layer")).clicked() {
                            acts.push(Act::Zoom(item.id));
                        }
                        if ui.button("Remove").on_hover_text("Takes the layer out of the list; the map file is not touched").clicked() {
                            acts.push(Act::Remove(item.id));
                        }
                    });
                } else {
                    ui.label(RichText::new("Reading the map ...").small().weak());
                }
            }
            Some(item) if item.kind == Kind::Vector => {
                ui.label(RichText::new(&item.name).strong());
                if let Some(v) = g.vectors.iter_mut().find(|v| v.id == item.id) {
                    let file = v.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui.label(RichText::new(if v.table.is_empty() { file } else { format!("{file}, table {}", v.table) }).small().weak());
                    ui.label(RichText::new(format!("Polygons (n={}), lines (n={}), points (n={})", v.data.polys.len(), v.data.lines.len(), v.data.points.len())).small().weak());
                    ui.label(RichText::new(&v.note).small().weak());
                    if v.crs.is_none() {
                        ui.label(RichText::new("Which coordinate system are the coordinates of the file in?").strong().small());
                        crs_picker(ui, "gis-vec-pending", &mut g.pending_crs);
                        if ui.button("Use this system").clicked() {
                            acts.push(Act::UseVecSystem(item.id));
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Colour");
                        ui.color_edit_button_srgb(&mut v.color);
                    });
                    super::thin_slider(ui, egui::Slider::new(&mut v.psize, 0.4..=3.0).text("point size").show_value(false));
                    super::thin_slider(ui, egui::Slider::new(&mut v.lwidth, 0.4..=4.0).text("line width").show_value(false));
                    // labels and the filter need the attributes of the file
                    if v.data.fields.is_empty() {
                        ui.label(RichText::new("This file has no attributes: no labels, no filter.").small().weak());
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Labels");
                            let shown = if v.label.is_empty() { "(none)".to_string() } else { v.label.clone() };
                            egui::ComboBox::from_id_salt("gis-vec-label").selected_text(shown).width(150.0).show_ui(ui, |ui| {
                                if ui.selectable_label(v.label.is_empty(), "(none)").clicked() {
                                    acts.push(Act::SetLabel(item.id, String::new()));
                                }
                                for f in &v.data.fields {
                                    if ui.selectable_label(&v.label == f, f).clicked() {
                                        acts.push(Act::SetLabel(item.id, f.clone()));
                                    }
                                }
                            });
                        });
                        let open = g.filter_focus == Some(item.id);
                        let mut header = egui::CollapsingHeader::new(RichText::new(if v.filter.trim().is_empty() { "Filter" } else { "Filter (active)" }).strong()).id_salt(("gis-filter", item.id)).default_open(!v.filter.trim().is_empty());
                        if open {
                            header = header.open(Some(true));
                        }
                        header.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("As in the QGIS Filter dialog").small());
                                info_icon(ui, "The condition is written like the SQL WHERE of a QGIS layer filter: \"FIELD\" IN ('a', 'b'), \"FIELD\" LIKE 'PRIB%', \"DEPTH\" > 100 AND \"TYPE\" = 'core', \"X\" IS NOT NULL, \"DIP\" BETWEEN 10 AND 45, NOT (...), + - * / %, || to join text, and the functions lower upper length trim abs round floor ceil coalesce concat substr left right to_int to_real. Field names go in double quotes, text in single quotes. A filter that comes from a QGIS project is filled in here and can be edited. Only the features that pass are drawn.");
                            });
                            let te = egui::TextEdit::multiline(&mut v.filter_draft).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY).hint_text("\"HOLEID\" IN ('PRB001', 'PRB002')");
                            let r = ui.add(te);
                            if open {
                                r.request_focus();
                            }
                            ui.horizontal_wrapped(|ui| {
                                let draft = v.filter_draft.clone();
                                if ui.add_enabled(draft != v.filter, egui::Button::new("Apply")).clicked() {
                                    let _ = v.set_filter(&draft);
                                }
                                if ui.add_enabled(!v.filter_draft.is_empty() || !v.filter.is_empty(), egui::Button::new("Clear")).clicked() {
                                    let _ = v.set_filter("");
                                }
                                egui::ComboBox::from_id_salt("gis-vec-field").selected_text("Insert field").width(110.0).show_ui(ui, |ui| {
                                    for f in v.data.fields.clone() {
                                        if ui.selectable_label(false, &f).clicked() {
                                            v.filter_draft.push_str(&format!("\"{f}\""));
                                        }
                                    }
                                });
                            });
                            if !v.filter_err.is_empty() {
                                ui.label(RichText::new(&v.filter_err).small().color(Color32::from_rgb(210, 70, 60)));
                            } else if v.filter_draft != v.filter {
                                ui.label(RichText::new("Not applied yet: press Apply.").small().weak());
                            }
                            ui.label(RichText::new(format!("Features shown: (n={}) of (n={})", v.matched.0, v.matched.1)).small().weak());
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(v.proj.is_some(), egui::Button::new("Zoom to layer")).clicked() {
                            acts.push(Act::Zoom(item.id));
                        }
                        if ui.button("Delete layer").on_hover_text("Takes the layer out of the list; the file is not touched").clicked() {
                            acts.push(Act::Remove(item.id));
                        }
                    });
                } else {
                    ui.label(RichText::new("Reading the file ...").small().weak());
                }
            }
            Some(item) if item.kind == Kind::Group => {
                ui.label(RichText::new(&item.name).strong());
                ui.label(RichText::new("A group: its tick box switches everything in it. Drag layers onto it.").small().weak());
                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked() {
                        acts.push(Act::Rename(item.id));
                    }
                    if ui.button("Remove group").on_hover_text("The layers of the group move up").clicked() {
                        acts.push(Act::Remove(item.id));
                    }
                });
            }
            Some(item) if item.kind == Kind::Basemap => {
                ui.label(RichText::new(&item.name).strong());
                ui.label(RichText::new(format!("{} from {}. {}. Off until ticked: the tiles are downloaded for the area you look at. Built in: it is always the bottom layer and can only be switched on and off; the Backdrop button above the map chooses between the street map and the satellite imagery.", g.tiles.source.short(), g.tiles.source.server(), g.tiles.source.attribution())).small().weak());
            }
            Some(item) => {
                ui.label(RichText::new(&item.name).strong());
                ui.label(RichText::new("One symbol per specimen with a place. Built in: it is always the top layer and can only be switched on and off.").small().weak());
                if ui.button("Symbol settings ...").clicked() {
                    acts.push(Act::SymbolSettings);
                }
            }
        }

        });
        // the filter box has been opened and focused for this frame
        self.gis.filter_focus = None;
        // ---- what was asked
        for a in acts {
            match a {
                Act::Select(id) => {
                    self.gis.sel = Some(id);
                    self.gis.multi.clear();
                    self.gis.multi_anchor = Some(id);
                }
                Act::SelectMods(id, shift, ctrl) => {
                    let order: Vec<u32> = self.gis.layers.rows().iter().map(|&(i, _)| self.gis.layers.items[i].id).collect();
                    if shift {
                        let a = self.gis.multi_anchor.or(self.gis.sel).and_then(|a| order.iter().position(|x| *x == a)).unwrap_or(0);
                        let b = order.iter().position(|x| *x == id).unwrap_or(a);
                        let (lo, hi) = (a.min(b), a.max(b));
                        self.gis.multi = order[lo..=hi].to_vec();
                    } else if ctrl {
                        if self.gis.multi.is_empty() {
                            if let Some(s) = self.gis.sel.filter(|s| order.contains(s)) {
                                self.gis.multi.push(s);
                            }
                        }
                        if let Some(pos) = self.gis.multi.iter().position(|x| *x == id) {
                            self.gis.multi.remove(pos);
                        } else {
                            self.gis.multi.push(id);
                        }
                        self.gis.multi_anchor = Some(id);
                    }
                    self.gis.sel = Some(id);
                }
                Act::SelectAll => {
                    let order: Vec<u32> = self.gis.layers.rows().iter().map(|&(i, _)| self.gis.layers.items[i].id).filter(|id| self.gis.layers.get(*id).map_or(false, |i| !i.kind.is_builtin())).collect();
                    self.gis.multi = order;
                }
                Act::RemoveMany(ids) => {
                    self.gis.snapshot_layers(format!("removed {} layer(s)", ids.len()));
                    let mut n = 0;
                    for id in ids {
                        if self.gis.layers.get(id).is_none() {
                            continue;
                        }
                        let gone = self.gis.layers.remove_subtree(id);
                        n += gone.len().max(1);
                        for g in gone {
                            self.gis.maps.retain(|m| m.id != g);
                            self.gis.restore_maps.remove(&g);
                            self.gis.vectors.retain(|v| v.id != g);
                            self.gis.restore_vecs.remove(&g);
                        }
                    }
                    self.gis.multi.clear();
                    self.gis.sel = self.gis.sel.filter(|s| self.gis.layers.get(*s).is_some());
                    self.gis.info = format!("Removed layers (n={n}) from the list. Ctrl+Z over the list puts them back; no file was changed.");
                }
                Act::Undo => {
                    self.gis.info = match self.gis.undo_layers(&ctx) {
                        Some(w) => format!("Undone: {w}."),
                        None => "Nothing to undo.".to_string(),
                    };
                }
                Act::ToggleVisible(id) => {
                    self.gis.sel = Some(id);
                    if self.gis.layers.get(id).map(|i| i.kind) == Some(Kind::Basemap) {
                        self.gis.base_on = !self.gis.base_on;
                    } else if let Some(i) = self.gis.layers.get_mut(id) {
                        i.visible = !i.visible;
                    }
                }
                Act::ToggleOpen(id) => {
                    if let Some(i) = self.gis.layers.get_mut(id) {
                        i.open = !i.open;
                    }
                }
                Act::Rename(id) => {
                    // the built-in layers keep their names
                    if self.gis.layers.get(id).map_or(false, |i| !i.kind.is_builtin()) {
                        self.gis.sel = Some(id);
                        self.gis.renaming = self.gis.layers.get(id).map(|i| (id, i.name.clone()));
                    }
                }
                Act::Georeference(id) => self.gis_open_georef(id),
                Act::Zoom(id) => {
                    let ext = self.gis.maps.iter().find(|m| m.id == id).and_then(|m| m.extent()).or_else(|| self.gis.vectors.iter().find(|v| v.id == id).and_then(|v| v.extent()));
                    if let Some((a, b)) = ext {
                        self.gis.view = View::fit(a, b, self.gis.canvas);
                        self.gis.fit_pending = false;
                    }
                }
                Act::Remove(id) => {
                    let what = self.gis.layers.get(id).map(|i| format!("removed \"{}\"", i.name)).unwrap_or_else(|| "removed a layer".into());
                    self.gis.snapshot_layers(what);
                    self.gis.multi.clear();
                    for gone in self.gis.layers.remove(id) {
                        self.gis.maps.retain(|m| m.id != gone);
                        self.gis.restore_maps.remove(&gone);
                        self.gis.vectors.retain(|v| v.id != gone);
                        self.gis.restore_vecs.remove(&gone);
                    }
                    if self.gis.layers.get(id).is_none() && self.gis.sel == Some(id) {
                        self.gis.sel = None;
                    }
                }
                Act::AddMap => {
                    if let Some(p) = pick_file("Map image, GeoTIFF, shapefile, GeoPackage or GeoJSON", "Maps and vector data", &["png", "jpg", "jpeg", "tif", "tiff", "shp", "gpkg", "geojson", "json"], "AMS_STUDIO_GIS_MAP") {
                        self.gis.load_map_file(&ctx, p);
                    }
                }
                Act::ImportQgis => {
                    if let Some(p) = pick_file("QGIS or ArcGIS project", "QGIS and ArcGIS projects", &["qgz", "qgs", "aprx", "lyrx", "mapx", "mxd", "lyr"], "AMS_STUDIO_GIS_QGIS") {
                        // the project file is only read; the layers it adds can be undone as one block
                        self.gis.snapshot_layers("added the layers of a project");
                        if let Err(e) = self.gis.import_qgis(&p) {
                            self.gis.layer_undo.pop();
                            self.gis.info.clear();
                            self.gis.map_err = e;
                        }
                    }
                }
                Act::AddGroup => {
                    let id = self.gis.layers.add_group("Group");
                    self.gis.sel = Some(id);
                    self.gis.renaming = Some((id, "Group".to_string()));
                }
                Act::Move(id, target, drop) => {
                    self.gis.layers.move_item(id, target, drop);
                }
                Act::UseSystem(id) => match self.gis.pending_crs.crs() {
                    Ok(crs) => {
                        if let Some(m) = self.gis.map_mut(id) {
                            if let Some(a) = m.pending.take() {
                                m.georef = Some(Georef { affine: a, crs });
                                m.note = format!("Georeference from the file, in {}.", crs.label());
                            }
                        }
                        self.gis.fit_pending = true;
                    }
                    Err(e) => self.gis.map_err = e,
                },
                Act::SymbolSettings => self.open_win(W_GIS),
                Act::PopOut => self.toggle_popout(),
                Act::RemoveAll(id) => {
                    let what = self.gis.layers.get(id).map(|i| format!("removed \"{}\" with its layers", i.name)).unwrap_or_else(|| "removed a group".into());
                    self.gis.snapshot_layers(what);
                    self.gis.multi.clear();
                    for gone in self.gis.layers.remove_subtree(id) {
                        self.gis.maps.retain(|m| m.id != gone);
                        self.gis.restore_maps.remove(&gone);
                        self.gis.vectors.retain(|v| v.id != gone);
                        self.gis.restore_vecs.remove(&gone);
                    }
                    if self.gis.sel.map_or(false, |s| self.gis.layers.get(s).is_none()) {
                        self.gis.sel = None;
                    }
                }
                Act::FocusFilter(id) => {
                    self.gis.sel = Some(id);
                    self.gis.filter_focus = Some(id);
                }
                Act::SetLabel(id, f) => {
                    if let Some(v) = self.gis.vectors.iter_mut().find(|v| v.id == id) {
                        v.label = f;
                    }
                }
                Act::MoveTop(id) => {
                    let first = self.gis.layers.items.iter().find(|i| !i.kind.is_builtin() && i.parent.is_none()).map(|i| i.id);
                    if let Some(t) = first {
                        self.gis.layers.move_item(id, t, Drop::Before);
                    }
                }
                Act::MoveBottom(id) => {
                    let last = self.gis.layers.items.iter().rev().find(|i| !i.kind.is_builtin() && i.parent.is_none()).map(|i| i.id);
                    if let Some(t) = last {
                        self.gis.layers.move_item(id, t, Drop::After);
                    }
                }
                Act::Duplicate(id) => {
                    // a copy of a vector layer right above the original, with its filter and look (handy for another
                    // filter of the same file); the name is up for renaming
                    if let Some(src) = self.gis.vectors.iter().find(|v| v.id == id) {
                        let copy = src.duplicate();
                        let name = self.gis.layers.get(id).map(|i| format!("{} copy", i.name)).unwrap_or_else(|| "Copy".into());
                        let nid = self.gis.layers.add_vector(&name);
                        self.gis.layers.move_item(nid, id, Drop::Before);
                        let mut copy = copy;
                        copy.id = nid;
                        self.gis.vectors.push(copy);
                        self.gis.sel = Some(nid);
                        self.gis.renaming = Some((nid, name));
                    }
                }
                Act::UseVecSystem(id) => match self.gis.pending_crs.crs() {
                    Ok(crs) => {
                        if let Some(v) = self.gis.vectors.iter_mut().find(|v| v.id == id) {
                            v.set_crs(crs);
                            v.note = format!("Coordinate system chosen: {}.", crs.label());
                        }
                        self.gis.fit_pending = true;
                    }
                    Err(e) => self.gis.map_err = e,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<(u32, Rect)> {
        (0..4).map(|i| (i as u32 + 10, Rect::from_min_size(Pos2::new(0.0, i as f32 * 28.0), Vec2::new(200.0, 28.0)))).collect()
    }

    #[test]
    fn a_drop_lands_before_after_or_inside_by_where_the_pointer_is() {
        let r = rows();
        let group = |id: u32| id == 11;
        // upper quarter, lower quarter
        assert_eq!(drop_for(&r, Pos2::new(50.0, 28.0 + 3.0), &group), Some((11, Drop::Before)));
        assert_eq!(drop_for(&r, Pos2::new(50.0, 28.0 + 26.0), &group), Some((11, Drop::After)));
        // the middle of a group is into it, the middle of a layer is before or after by halves
        assert_eq!(drop_for(&r, Pos2::new(50.0, 28.0 + 14.0), &group), Some((11, Drop::Inside)));
        assert_eq!(drop_for(&r, Pos2::new(50.0, 56.0 + 10.0), &group), Some((12, Drop::Before)));
        assert_eq!(drop_for(&r, Pos2::new(50.0, 56.0 + 18.0), &group), Some((12, Drop::After)));
        // above the list and below it
        assert_eq!(drop_for(&r, Pos2::new(50.0, -20.0), &group), Some((10, Drop::Before)));
        assert_eq!(drop_for(&r, Pos2::new(50.0, 500.0), &group), Some((13, Drop::After)));
        assert_eq!(drop_for(&[], Pos2::ZERO, &group), None);
    }
}
