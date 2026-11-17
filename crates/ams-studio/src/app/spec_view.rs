//! Sort and filter of the specimens pane. This only changes how the list is shown (and the order Previous / Next
//! and the Data Table follow); the records in the file, their order in it and the plots are not touched.
//!
//! The ID sorts are the ones of the recombine tool (`ams_core::recombine::order_cmp`): samples stay together and are
//! read like a person would (ZZ2 before ZZ10); inside a sample the sub-specimens either run through the letters for
//! each number (A1 B1 C1 D1, A2 B2 ...) or through the numbers for each letter (A1 A2 A3 A4, B1 B2 ...).

use super::{info_icon, tools_ui, App};
use crate::doc::Doc;
use ams_core::params::{param_values_vol, N_PARAMS, PARAM_LABELS};
use ams_core::recombine::{natural_cmp, order_cmp, Within};
use egui::{Pos2, RichText, Sense, Stroke, Vec2};
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKind {
    /// As the records are stored in the file.
    File,
    /// A1 B1 C1 D1 A2 B2 ...
    NumberThenLetter,
    /// A1 A2 A3 A4 B1 B2 ...
    LetterThenNumber,
    /// The whole name, numbers read as numbers (ZZ2 before ZZ10).
    Natural,
    /// By the value of one of the plottable parameters.
    Param(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    All,
    Selected,
    NotSelected,
    Active,
    Disregarded,
}

impl Show {
    const ALL: [(Show, &'static str); 5] = [
        (Show::All, "All specimens"),
        (Show::Selected, "Selected only"),
        (Show::NotSelected, "Not selected"),
        (Show::Active, "Active only (not disregarded)"),
        (Show::Disregarded, "Disregarded only"),
    ];
}

#[derive(Clone, PartialEq, Debug)]
pub struct SpecView {
    pub kind: SortKind,
    pub descending: bool,
    /// Disregarded specimens stay below the active ones (the list's own rule, on by default).
    pub bottom_disregarded: bool,
    /// Words separated by spaces or commas; a name is listed when it contains any of them; `-word` leaves out the
    /// names that contain it.
    pub text: String,
    pub show: Show,
    pub range_on: bool,
    pub range_param: usize,
    pub min_text: String,
    pub max_text: String,
}

impl Default for SpecView {
    fn default() -> Self {
        SpecView {
            kind: SortKind::File,
            descending: false,
            bottom_disregarded: true,
            text: String::new(),
            show: Show::All,
            range_on: false,
            range_param: 4,
            min_text: String::new(),
            max_text: String::new(),
        }
    }
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl SpecView {
    /// The text split into words to look for and words to leave out (lower case).
    fn words(&self) -> (Vec<String>, Vec<String>) {
        let (mut inc, mut exc) = (vec![], vec![]);
        for w in self.text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()) {
            match w.strip_prefix('-') {
                Some(x) if !x.is_empty() => exc.push(x.to_lowercase()),
                Some(_) => {}
                None => inc.push(w.to_lowercase()),
            }
        }
        (inc, exc)
    }

    fn range(&self) -> Option<(Option<f64>, Option<f64>)> {
        if !self.range_on {
            return None;
        }
        Some((parse_num(&self.min_text), parse_num(&self.max_text)))
    }

    /// Anything is filtered away.
    pub fn is_filtering(&self) -> bool {
        !self.text.trim().is_empty() || self.show != Show::All || self.range().is_some_and(|(a, b)| a.is_some() || b.is_some())
    }

    /// Anything differs from the plain list.
    pub fn is_active(&self) -> bool {
        self.kind != SortKind::File || self.is_filtering()
    }

    /// Whether the sort needs the values of a parameter.
    pub fn sort_param(&self) -> Option<usize> {
        if let SortKind::Param(p) = self.kind {
            Some(p)
        } else {
            None
        }
    }

    /// Whether the filter needs the values of a parameter (and which).
    pub fn range_param_in_use(&self) -> Option<usize> {
        self.range().filter(|(a, b)| a.is_some() || b.is_some()).map(|_| self.range_param)
    }
}

/// What the list needs to know about the file, per specimen (all slices as long as the file).
pub struct Facts<'a> {
    pub names: &'a [String],
    pub active: &'a [bool],
    pub selected: &'a [bool],
    /// Values of the sort parameter, empty when the sort is not by a parameter.
    pub sort_vals: &'a [f64],
    /// Values of the filter parameter, empty when there is no range filter.
    pub range_vals: &'a [f64],
}

/// The specimens to list, in the order to list them.
pub fn view_order(f: &Facts, sv: &SpecView) -> Vec<usize> {
    let n = f.names.len();
    let (inc, exc) = sv.words();
    let range = sv.range_param_in_use().and(sv.range());
    let mut idx: Vec<usize> = (0..n)
        .filter(|&i| {
            let name = f.names[i].to_lowercase();
            if !inc.is_empty() && !inc.iter().any(|w| name.contains(w.as_str())) {
                return false;
            }
            if exc.iter().any(|w| name.contains(w.as_str())) {
                return false;
            }
            let ok = match sv.show {
                Show::All => true,
                Show::Selected => f.selected[i],
                Show::NotSelected => !f.selected[i],
                Show::Active => f.active[i],
                Show::Disregarded => !f.active[i],
            };
            if !ok {
                return false;
            }
            if let Some((lo, hi)) = range {
                let v = f.range_vals.get(i).copied().unwrap_or(f64::NAN);
                if !v.is_finite() || lo.is_some_and(|l| v < l) || hi.is_some_and(|h| v > h) {
                    return false;
                }
            }
            true
        })
        .collect();
    let dir = |o: Ordering| if sv.descending { o.reverse() } else { o };
    match sv.kind {
        SortKind::File => {
            if sv.descending {
                idx.reverse();
            }
        }
        SortKind::NumberThenLetter => idx.sort_by(|&a, &b| dir(order_cmp(&f.names[a], &f.names[b], Within::NumberThenLetter))),
        SortKind::LetterThenNumber => idx.sort_by(|&a, &b| dir(order_cmp(&f.names[a], &f.names[b], Within::LetterThenNumber))),
        SortKind::Natural => idx.sort_by(|&a, &b| dir(natural_cmp(&f.names[a], &f.names[b]))),
        SortKind::Param(_) => idx.sort_by(|&a, &b| {
            let (x, y) = (f.sort_vals.get(a).copied().unwrap_or(f64::NAN), f.sort_vals.get(b).copied().unwrap_or(f64::NAN));
            // specimens without a value always go last, whichever way the sort runs
            match (x.is_finite(), y.is_finite()) {
                (true, true) => dir(x.partial_cmp(&y).unwrap_or(Ordering::Equal)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => Ordering::Equal,
            }
        }),
    }
    if sv.bottom_disregarded {
        let (mut act, dis): (Vec<usize>, Vec<usize>) = idx.into_iter().partition(|&i| f.active[i]);
        act.extend(dis);
        idx = act;
    }
    idx
}

/// The list order for an open document: used by the specimens pane, Previous / Next and the Data Table.
pub fn doc_order(doc: &Doc, sv: &SpecView, sort_vals: &[f64], range_vals: &[f64]) -> Vec<usize> {
    if !sv.is_active() && sv.bottom_disregarded {
        return doc.order();
    }
    let names: Vec<String> = doc.specs.iter().map(|s| s.name.clone()).collect();
    view_order(&Facts { names: &names, active: &doc.active, selected: &doc.selected, sort_vals, range_vals }, sv)
}

/// The sort and filter button of the specimens pane header: a funnel (filter) with up / down arrows (sort), drawn
/// with the painter. `active` colours it with the accent colour while a sort or filter is on.
pub(super) fn sort_filter_button(ui: &mut egui::Ui, active: bool, hint: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(34.0, 22.0), Sense::click());
    let v = ui.visuals();
    let fg = if active { v.hyperlink_color } else if resp.hovered() { v.strong_text_color() } else { v.text_color() };
    if resp.hovered() || active {
        let bg = if active { fg.gamma_multiply(0.18) } else { v.widgets.hovered.bg_fill };
        ui.painter().rect_filled(rect, 4.0, bg);
    }
    let p = ui.painter();
    let o = rect.min + Vec2::new(4.0, 3.5);
    // funnel: a wide top, a cone and a short stem
    let funnel = vec![o, o + Vec2::new(14.0, 0.0), o + Vec2::new(9.0, 7.0), o + Vec2::new(9.0, 14.0), o + Vec2::new(5.0, 12.0), o + Vec2::new(5.0, 7.0)];
    p.add(egui::Shape::convex_polygon(funnel, fg.gamma_multiply(0.85), Stroke::new(1.0, fg)));
    // sort arrows: one up, one down
    let ax = rect.max.x - 7.0;
    let st = Stroke::new(1.5, fg);
    let (top, bot) = (rect.min.y + 4.5, rect.max.y - 4.5);
    p.line_segment([Pos2::new(ax - 3.0, top), Pos2::new(ax - 3.0, bot)], st);
    p.line_segment([Pos2::new(ax - 5.5, top + 2.5), Pos2::new(ax - 3.0, top)], st);
    p.line_segment([Pos2::new(ax - 0.5, top + 2.5), Pos2::new(ax - 3.0, top)], st);
    p.line_segment([Pos2::new(ax + 3.0, top), Pos2::new(ax + 3.0, bot)], st);
    p.line_segment([Pos2::new(ax + 0.5, bot - 2.5), Pos2::new(ax + 3.0, bot)], st);
    p.line_segment([Pos2::new(ax + 5.5, bot - 2.5), Pos2::new(ax + 3.0, bot)], st);
    resp.on_hover_text(hint)
}

impl App {
    /// The values of one parameter for every specimen of the open file, as plotted (current phase and system).
    fn param_column(&self, p: usize) -> Vec<f64> {
        let Some(doc) = &self.doc else { return vec![] };
        if self.derived.len() != doc.specs.len() || p >= N_PARAMS {
            return vec![];
        }
        (0..doc.specs.len())
            .map(|i| {
                let d = &self.derived[i];
                let ph = if self.out_phase { &doc.specs[i].outphase } else { &doc.specs[i].inphase };
                let vol = if doc.specs[i].volume_cm3 > 0.0 { doc.specs[i].volume_cm3 } else { ams_core::params::REF_VOLUME };
                param_values_vol(d.km, &d.axes, ph, vol)[p]
            })
            .collect()
    }

    /// The specimens in the order the specimens pane lists them (sort and filter applied).
    pub(super) fn list_order(&self) -> Vec<usize> {
        let Some(doc) = &self.doc else { return vec![] };
        let sort_vals = self.sv.sort_param().map(|p| self.param_column(p)).unwrap_or_default();
        let range_vals = self.sv.range_param_in_use().map(|p| self.param_column(p)).unwrap_or_default();
        doc_order(doc, &self.sv, &sort_vals, &range_vals)
    }

    pub(super) fn sort_window(&mut self, ctx: &egui::Context) {
        if !self.win_sort {
            return;
        }
        let mut open = true;
        let mut sv = self.sv.clone();
        let preview: Vec<String> = match &self.doc {
            Some(d) => self.list_order().into_iter().take(8).map(|i| d.specs[i].name.clone()).collect(),
            None => vec![],
        };
        let (shown, total) = (self.list_order().len(), self.doc.as_ref().map_or(0, |d| d.specs.len()));
        let mut close = false;
        self.base_window(ctx, "Sort and filter specimens", super::W_SORT, Vec2::new(600.0, 640.0)).open(&mut open).resizable(true).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.2);
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 9.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("How should the specimens be sorted?").strong().size(19.0));
                info_icon(ui, "Only the list in the sub-specimens pane (and Previous / Next and the Data Table, which follow it) is sorted. The records in the file, their order in it, the plots and the saved files are not changed. The two ID sorts are the ones of the recombine tool: samples stay together (ZZ2 before ZZ10), and inside a sample the sub-specimens run through the letters for each number (A1 B1 C1 D1, then A2 B2 ...) or through the numbers for each letter (A1 A2 A3 A4, then B1 B2 ...). Names that do not end in letters and a number keep their place from the file.");
            });
            ui.radio_value(&mut sv.kind, SortKind::File, "As stored in the file");
            ui.radio_value(&mut sv.kind, SortKind::NumberThenLetter, "A1 B1 C1 D1 ... then A2 B2 C2 D2 (through the letters for each number)");
            ui.radio_value(&mut sv.kind, SortKind::LetterThenNumber, "A1 A2 A3 A4 ... then B1 B2 B3 B4 (through the numbers for each letter)");
            ui.radio_value(&mut sv.kind, SortKind::Natural, "By the whole name (numbers read as numbers)");
            ui.horizontal(|ui| {
                let mut by_param = matches!(sv.kind, SortKind::Param(_));
                if ui.radio(by_param, "By the value of").clicked() && !by_param {
                    sv.kind = SortKind::Param(sv.range_param);
                    by_param = true;
                }
                ui.add_enabled_ui(by_param, |ui| {
                    let mut p = if let SortKind::Param(p) = sv.kind { p } else { sv.range_param };
                    egui::ComboBox::from_id_salt("sv_sort_param").selected_text(PARAM_LABELS[p]).show_ui(ui, |ui| {
                        for (k, l) in PARAM_LABELS.iter().enumerate() {
                            ui.selectable_value(&mut p, k, *l);
                        }
                    });
                    if by_param {
                        sv.kind = SortKind::Param(p);
                    }
                });
                info_icon(ui, "The parameter as the plots show it (the phase and coordinate system that are selected, volume corrected where the parameter is). Specimens without a value are listed last.");
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut sv.descending, false, "Ascending");
                ui.selectable_value(&mut sv.descending, true, "Descending");
            });
            ui.checkbox(&mut sv.bottom_disregarded, "Disregarded specimens stay at the bottom");

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Which specimens should be listed?").strong().size(19.0));
                info_icon(ui, "Only hides rows of the list; the hidden specimens stay in the file, keep their selection and are still plotted. Name: words separated by spaces or commas, a specimen is listed when its name contains any of them (capitals do not matter); a word with a minus in front leaves out the names that contain it, for example: A1 B1 -ZZ05.");
            });
            ui.horizontal(|ui| {
                ui.label("Name contains");
                ui.add(egui::TextEdit::singleline(&mut sv.text).desired_width(260.0).hint_text("A1 B1 -ZZ05"));
            });
            ui.horizontal(|ui| {
                ui.label("Show");
                egui::ComboBox::from_id_salt("sv_show").selected_text(Show::ALL.iter().find(|s| s.0 == sv.show).map_or("", |s| s.1)).show_ui(ui, |ui| {
                    for (s, l) in Show::ALL {
                        ui.selectable_value(&mut sv.show, s, l);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut sv.range_on, "Only where");
                ui.add_enabled_ui(sv.range_on, |ui| {
                    egui::ComboBox::from_id_salt("sv_range_param").selected_text(PARAM_LABELS[sv.range_param]).show_ui(ui, |ui| {
                        for (k, l) in PARAM_LABELS.iter().enumerate() {
                            ui.selectable_value(&mut sv.range_param, k, *l);
                        }
                    });
                    ui.label("is from");
                    ui.add(egui::TextEdit::singleline(&mut sv.min_text).desired_width(70.0).hint_text("min"));
                    ui.label("to");
                    ui.add(egui::TextEdit::singleline(&mut sv.max_text).desired_width(70.0).hint_text("max"));
                });
                info_icon(ui, "Keeps the specimens whose value of the parameter lies in the range; leave one side empty for no limit. Specimens without a value are hidden while a limit is set.");
            });
            if sv.range_on && (!sv.min_text.trim().is_empty() && parse_num(&sv.min_text).is_none() || !sv.max_text.trim().is_empty() && parse_num(&sv.max_text).is_none()) {
                ui.label(RichText::new("The limits have to be numbers (a point or a comma is fine).").small().color(egui::Color32::from_rgb(200, 90, 40)));
            }

            ui.separator();
            if self.doc.is_some() {
                ui.label(RichText::new(format!("Listed (n={shown}) of (n={total})")).strong());
                ui.label(RichText::new(if preview.is_empty() { "nothing matches".to_string() } else { format!("First in the list: {}{}", preview.join("  "), if shown > preview.len() { "  ..." } else { "" }) }).weak().small());
            } else {
                ui.label(RichText::new("No file open.").weak());
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Back to the plain list, or done?").strong());
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Reset sort and filter").min_size(Vec2::new(190.0, 34.0))).clicked() {
                    sv = SpecView::default();
                }
                if ui.add(egui::Button::new("Close").min_size(Vec2::new(110.0, 34.0))).clicked() {
                    close = true;
                }
            });
        });
        if sv != self.sv {
            self.sv = sv;
            self.scroll_to = None;
        }
        self.win_sort = open && !close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn run(n: &[&str], active: &[bool], selected: &[bool], sv: &SpecView, sort_vals: &[f64], range_vals: &[f64]) -> Vec<String> {
        let nm = names(n);
        view_order(&Facts { names: &nm, active, selected, sort_vals, range_vals }, sv).into_iter().map(|i| nm[i].clone()).collect()
    }

    const CUBES: [&str; 6] = ["ZZ01-A2", "ZZ01-B1", "ZZ01-A1", "ZZ01-B2", "ZZ01-A3", "ZZ01-B3"];

    fn plain(n: usize) -> (Vec<bool>, Vec<bool>) {
        (vec![true; n], vec![false; n])
    }

    #[test]
    fn id_sorts_are_the_recombine_orders() {
        let (a, s) = plain(6);
        let mut sv = SpecView::default();
        sv.kind = SortKind::NumberThenLetter;
        assert_eq!(run(&CUBES, &a, &s, &sv, &[], &[]), ["ZZ01-A1", "ZZ01-B1", "ZZ01-A2", "ZZ01-B2", "ZZ01-A3", "ZZ01-B3"]);
        sv.kind = SortKind::LetterThenNumber;
        assert_eq!(run(&CUBES, &a, &s, &sv, &[], &[]), ["ZZ01-A1", "ZZ01-A2", "ZZ01-A3", "ZZ01-B1", "ZZ01-B2", "ZZ01-B3"]);
        sv.descending = true;
        assert_eq!(run(&CUBES, &a, &s, &sv, &[], &[]), ["ZZ01-B3", "ZZ01-B2", "ZZ01-B1", "ZZ01-A3", "ZZ01-A2", "ZZ01-A1"]);
    }

    #[test]
    fn plain_names_a1_b1_c1_sort_in_the_two_different_orders() {
        // records named only by their cube: letter and number, no sample name in front
        let n = ["A2", "B1", "A1", "B2", "C1", "D1", "C2", "D2", "A10", "A3"];
        let (a, s) = plain(10);
        let mut sv = SpecView::default();
        sv.kind = SortKind::NumberThenLetter;
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["A1", "B1", "C1", "D1", "A2", "B2", "C2", "D2", "A3", "A10"]);
        sv.kind = SortKind::LetterThenNumber;
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["A1", "A2", "A3", "A10", "B1", "B2", "C1", "C2", "D1", "D2"]);
        sv.descending = true;
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["D2", "D1", "C2", "C1", "B2", "B1", "A10", "A3", "A2", "A1"]);
    }

    #[test]
    fn samples_stay_together_in_natural_order() {
        let n = ["ZZ10-A1", "ZZ2-B1", "ZZ2-A1", "ZZ10-B1"];
        let (a, s) = plain(4);
        let mut sv = SpecView::default();
        sv.kind = SortKind::NumberThenLetter;
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["ZZ2-A1", "ZZ2-B1", "ZZ10-A1", "ZZ10-B1"]);
        sv.kind = SortKind::Natural;
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["ZZ2-A1", "ZZ2-B1", "ZZ10-A1", "ZZ10-B1"]);
    }

    #[test]
    fn file_order_and_its_reverse() {
        let (a, s) = plain(3);
        let mut sv = SpecView::default();
        assert_eq!(run(&["c", "a", "b"], &a, &s, &sv, &[], &[]), ["c", "a", "b"]);
        sv.descending = true;
        assert_eq!(run(&["c", "a", "b"], &a, &s, &sv, &[], &[]), ["b", "a", "c"]);
    }

    #[test]
    fn disregarded_specimens_go_last_unless_told_otherwise() {
        let n = ["A1", "B1", "C1"];
        let act = [true, false, true];
        let s = [false; 3];
        let mut sv = SpecView::default();
        sv.kind = SortKind::Natural;
        assert_eq!(run(&n, &act, &s, &sv, &[], &[]), ["A1", "C1", "B1"]);
        sv.bottom_disregarded = false;
        assert_eq!(run(&n, &act, &s, &sv, &[], &[]), ["A1", "B1", "C1"]);
    }

    #[test]
    fn name_filter_words_include_and_exclude() {
        let n = ["ZZ01-A1", "ZZ01-B1", "ZZ05-A1", "ZZ05-B1"];
        let (a, s) = plain(4);
        let mut sv = SpecView::default();
        sv.text = "A1, B1 -zz05".into();
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["ZZ01-A1", "ZZ01-B1"]);
        sv.text = "-ZZ01".into();
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["ZZ05-A1", "ZZ05-B1"]);
        sv.text = "a1".into();
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]), ["ZZ01-A1", "ZZ05-A1"]);
        sv.text = "-".into();
        assert_eq!(run(&n, &a, &s, &sv, &[], &[]).len(), 4);
    }

    #[test]
    fn show_modes() {
        let n = ["a", "b", "c", "d"];
        let act = [true, true, false, true];
        let sel = [true, false, true, false];
        let mut sv = SpecView::default();
        sv.show = Show::Selected;
        assert_eq!(run(&n, &act, &sel, &sv, &[], &[]), ["a", "c"]);
        sv.show = Show::NotSelected;
        assert_eq!(run(&n, &act, &sel, &sv, &[], &[]), ["b", "d"]);
        sv.show = Show::Active;
        assert_eq!(run(&n, &act, &sel, &sv, &[], &[]), ["a", "b", "d"]);
        sv.show = Show::Disregarded;
        assert_eq!(run(&n, &act, &sel, &sv, &[], &[]), ["c"]);
    }

    #[test]
    fn parameter_sort_puts_missing_values_last_both_ways() {
        let n = ["a", "b", "c", "d"];
        let (act, sel) = plain(4);
        let vals = [3.0, f64::NAN, 1.0, 2.0];
        let mut sv = SpecView::default();
        sv.kind = SortKind::Param(4);
        assert_eq!(run(&n, &act, &sel, &sv, &vals, &[]), ["c", "d", "a", "b"]);
        sv.descending = true;
        assert_eq!(run(&n, &act, &sel, &sv, &vals, &[]), ["a", "d", "c", "b"]);
    }

    #[test]
    fn range_filter_with_one_or_both_limits() {
        let n = ["a", "b", "c", "d"];
        let (act, sel) = plain(4);
        let vals = [1.0, 2.0, f64::NAN, 3.5];
        let mut sv = SpecView::default();
        sv.range_on = true;
        // no limit typed yet: nothing is filtered, not even the specimen without a value
        assert_eq!(run(&n, &act, &sel, &sv, &[], &vals).len(), 4);
        assert!(!sv.is_filtering());
        sv.min_text = "1,5".into();
        assert_eq!(run(&n, &act, &sel, &sv, &[], &vals), ["b", "d"]);
        sv.max_text = "3".into();
        assert_eq!(run(&n, &act, &sel, &sv, &[], &vals), ["b"]);
        sv.range_on = false;
        assert_eq!(run(&n, &act, &sel, &sv, &[], &vals).len(), 4);
    }

    #[test]
    fn default_view_is_not_active() {
        assert!(!SpecView::default().is_active());
        let mut sv = SpecView::default();
        sv.kind = SortKind::Natural;
        assert!(sv.is_active() && !sv.is_filtering());
    }
}
