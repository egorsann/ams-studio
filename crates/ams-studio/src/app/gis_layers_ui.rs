//! The layers sidebar of the GIS pane, like the layer list of QGIS: map layers, the OpenStreetMap basemap and the specimen
//! symbols, in groups. Tick boxes switch layers and groups on and off, a drag moves a layer or a group (before, after or
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
enum Act {
    Select(u32),
    ToggleVisible(u32),
    ToggleOpen(u32),
    Rename(u32),
    Georeference(u32),
    Zoom(u32),
    Remove(u32),
    AddMap,
    AddGroup,
    Move(u32, u32, Drop),
    UseSystem(u32),
    SymbolSettings,
    PopOut,
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
            info_icon(ui, "Like the layer list of QGIS. The tick box switches a layer or a whole group on and off; drag a layer or a group to move it (onto the upper or lower edge of a row to put it before or after, onto the middle of a group to put it inside); double-click renames; right-click has the rest. What is higher in the list is drawn above what is lower. Shift+Ctrl+B shows and hides this sidebar.");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Add group").clicked() {
                    acts.push(Act::AddGroup);
                }
                if ui.small_button("Add map ...").on_hover_text("A map image or a GeoTIFF as a new layer").clicked() {
                    acts.push(Act::AddMap);
                }
            });
        });

        // ---- the rows
        let rows = self.gis.layers.rows();
        let mut row_rects: Vec<(u32, Rect)> = vec![];
        {
            for (idx, depth) in rows.iter().copied() {
                let it = self.gis.layers.items[idx].clone();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
                row_rects.push((it.id, rect));
                let selected = self.gis.sel == Some(it.id);
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
                let shown = self.gis.layers.effective_visible(it.id) || (it.kind == Kind::Basemap && own_on && it.parent.map_or(true, |p| self.gis.layers.effective_visible(p)));
                paint_check(ui, cb, own_on, !shown && own_on, th.accent);
                let name_x = cb.max.x + 8.0;
                let name_rect = Rect::from_min_max(Pos2::new(name_x, rect.min.y + 2.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 2.0));
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
                    let g = ui.painter().layout_no_wrap(label.text().to_string(), egui::FontId::proportional(13.5), col);
                    let clip = ui.painter().with_clip_rect(name_rect);
                    clip.galley(Pos2::new(name_x, rect.center().y - g.size().y / 2.0), g, col);
                }
                if resp.clicked() {
                    let p = resp.interact_pointer_pos().unwrap_or(rect.center());
                    if it.kind == Kind::Group && arrow.expand(2.0).contains(p) {
                        acts.push(Act::ToggleOpen(it.id));
                    } else if cb.expand(3.0).contains(p) {
                        acts.push(Act::ToggleVisible(it.id));
                    } else {
                        acts.push(Act::Select(it.id));
                    }
                }
                if resp.double_clicked() && !cb.expand(3.0).contains(resp.interact_pointer_pos().unwrap_or(rect.center())) {
                    acts.push(Act::Rename(it.id));
                }
                if resp.drag_started() && !cb.expand(3.0).contains(resp.interact_pointer_pos().unwrap_or(rect.center())) && !renaming {
                    self.gis.layer_drag = Some(it.id);
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        acts.push(Act::Rename(it.id));
                        ui.close();
                    }
                    if it.kind == Kind::Map {
                        if ui.button("Georeference ...").clicked() {
                            acts.push(Act::Georeference(it.id));
                            ui.close();
                        }
                        if ui.button("Zoom to layer").clicked() {
                            acts.push(Act::Zoom(it.id));
                            ui.close();
                        }
                    }
                    if matches!(it.kind, Kind::Map | Kind::Group) && ui.button(if it.kind == Kind::Map { "Remove layer" } else { "Remove group (its layers move up)" }).clicked() {
                        acts.push(Act::Remove(it.id));
                        ui.close();
                    }
                });
            }
        }

        // ---- a drag in progress: where it would land, and the drop
        if let Some(dragged) = self.gis.layer_drag {
            let pointer = ui.input(|i| i.pointer.latest_pos());
            let is_group = |id: u32| self.gis.layers.get(id).map_or(false, |i| i.kind == Kind::Group);
            let target = pointer.filter(|p| p.x >= rect.min.x && p.x <= rect.max.x).and_then(|p| drop_for(&row_rects, p, &is_group));
            if !ui.input(|i| i.pointer.primary_down()) {
                if let Some((t, d)) = target {
                    acts.push(Act::Move(dragged, t, d));
                }
                self.gis.layer_drag = None;
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                ui.ctx().request_repaint();
                if let Some((t, d)) = target {
                    if let Some((_, r)) = row_rects.iter().find(|(id, _)| *id == t) {
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
                    ui.add(egui::Slider::new(&mut m.opacity, 0.05..=1.0).text("opacity").show_value(false));
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
                ui.label(RichText::new("Map tiles from tile.openstreetmap.org, © OpenStreetMap contributors. Off until ticked: the tiles are downloaded for the area you look at.").small().weak());
            }
            Some(item) => {
                ui.label(RichText::new(&item.name).strong());
                ui.label(RichText::new("One symbol per specimen with a place.").small().weak());
                if ui.button("Symbol settings ...").clicked() {
                    acts.push(Act::SymbolSettings);
                }
            }
        }

        });
        // ---- what was asked
        for a in acts {
            match a {
                Act::Select(id) => self.gis.sel = Some(id),
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
                    self.gis.sel = Some(id);
                    self.gis.renaming = self.gis.layers.get(id).map(|i| (id, i.name.clone()));
                }
                Act::Georeference(id) => self.gis_open_georef(id),
                Act::Zoom(id) => {
                    let ext = self.gis.maps.iter().find(|m| m.id == id).and_then(|m| m.extent());
                    if let Some((a, b)) = ext {
                        self.gis.view = View::fit(a, b, self.gis.canvas);
                        self.gis.fit_pending = false;
                    }
                }
                Act::Remove(id) => {
                    for gone in self.gis.layers.remove(id) {
                        self.gis.maps.retain(|m| m.id != gone);
                        self.gis.restore_maps.remove(&gone);
                    }
                    if self.gis.layers.get(id).is_none() && self.gis.sel == Some(id) {
                        self.gis.sel = None;
                    }
                }
                Act::AddMap => {
                    if let Some(p) = pick_file("Geologic map image or GeoTIFF", "Image", &["png", "jpg", "jpeg", "tif", "tiff"], "AMS_STUDIO_GIS_MAP") {
                        self.gis.load_map_file(&ctx, p);
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
