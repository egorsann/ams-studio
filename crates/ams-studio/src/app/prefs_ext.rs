//! Text forms of the pane settings that are kept in the preferences file besides the ones `state.txt` always had:
//! the colour-coding of the panes, the sort and filter of the Sub-specimens list, and lists of switches. Every form
//! round-trips (tested), and a form that cannot be read is dropped, so an old or damaged file never breaks the start.

use super::spec_view::{Show, SortKind, SpecView};
use crate::plots::ColorCfg;

pub fn bits(v: &[bool]) -> String {
    v.iter().map(|b| if *b { '1' } else { '0' }).collect()
}

/// `n` switches from `1011`; `None` when the text is not `n` long.
pub fn from_bits(t: &str, n: usize) -> Option<Vec<bool>> {
    let v: Vec<bool> = t.trim().chars().map(|c| c == '1').collect();
    (v.len() == n && t.trim().chars().all(|c| c == '0' || c == '1')).then_some(v)
}

pub fn color_to_line(c: &ColorCfg) -> String {
    let clean = |s: &str| s.replace(['|', '\n', '\r'], " ");
    format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", c.on as u8, c.param, c.auto as u8, c.min, c.max, c.log as u8, c.palette, c.by_class as u8, c.hy as u8, clean(&c.hy_key), clean(&c.hy_label))
}

pub fn color_from_line(t: &str) -> Option<ColorCfg> {
    let p: Vec<&str> = t.split('|').collect();
    // 8 fields: written before the hyperspectral values existed
    if p.len() != 8 && p.len() != 11 {
        return None;
    }
    let flag = |s: &str| s == "1";
    Some(ColorCfg {
        on: flag(p[0]),
        param: p[1].parse().ok().filter(|v| *v < ams_core::params::N_PARAMS)?,
        auto: flag(p[2]),
        min: p[3].parse().ok().filter(|v: &f64| v.is_finite())?,
        max: p[4].parse().ok().filter(|v: &f64| v.is_finite())?,
        log: flag(p[5]),
        palette: p[6].parse().ok().filter(|v| *v <= 2)?,
        by_class: flag(p[7]),
        hy: p.get(8).map_or(false, |s| flag(s)),
        hy_key: p.get(9).map(|s| s.to_string()).unwrap_or_default(),
        hy_label: p.get(10).map(|s| s.to_string()).unwrap_or_default(),
    })
}

fn kind_token(k: SortKind) -> String {
    match k {
        SortKind::File => "file".into(),
        SortKind::NumberThenLetter => "number_letter".into(),
        SortKind::LetterThenNumber => "letter_number".into(),
        SortKind::Natural => "natural".into(),
        SortKind::Param(p) => format!("param{p}"),
    }
}

fn kind_from(t: &str) -> Option<SortKind> {
    Some(match t {
        "file" => SortKind::File,
        "number_letter" => SortKind::NumberThenLetter,
        "letter_number" => SortKind::LetterThenNumber,
        "natural" => SortKind::Natural,
        p => SortKind::Param(p.strip_prefix("param")?.parse().ok().filter(|v| *v < ams_core::params::N_PARAMS)?),
    })
}

fn show_token(s: Show) -> &'static str {
    match s {
        Show::All => "all",
        Show::Selected => "selected",
        Show::NotSelected => "not_selected",
        Show::Active => "active",
        Show::Disregarded => "disregarded",
    }
}

fn show_from(t: &str) -> Option<Show> {
    Some(match t {
        "all" => Show::All,
        "selected" => Show::Selected,
        "not_selected" => Show::NotSelected,
        "active" => Show::Active,
        "disregarded" => Show::Disregarded,
        _ => return None,
    })
}

pub fn sv_to_line(sv: &SpecView) -> String {
    serde_json::json!({
        "kind": kind_token(sv.kind), "descending": sv.descending, "bottom_disregarded": sv.bottom_disregarded, "text": sv.text,
        "show": show_token(sv.show), "range_on": sv.range_on, "range_param": sv.range_param, "min": sv.min_text, "max": sv.max_text,
    })
    .to_string()
}

pub fn sv_from_line(t: &str) -> Option<SpecView> {
    let v: serde_json::Value = serde_json::from_str(t).ok()?;
    let b = |k: &str, d: bool| v.get(k).and_then(|x| x.as_bool()).unwrap_or(d);
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(SpecView {
        kind: kind_from(v.get("kind")?.as_str()?)?,
        descending: b("descending", false),
        bottom_disregarded: b("bottom_disregarded", true),
        text: s("text"),
        show: show_from(v.get("show")?.as_str()?)?,
        range_on: b("range_on", false),
        range_param: v.get("range_param").and_then(|x| x.as_u64()).map(|x| x as usize).filter(|x| *x < ams_core::params::N_PARAMS).unwrap_or(4),
        min_text: s("min"),
        max_text: s("max"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_round_trip_and_reject_wrong_lengths() {
        assert_eq!(bits(&[true, false, true]), "101");
        assert_eq!(from_bits("101", 3), Some(vec![true, false, true]));
        assert_eq!(from_bits("10", 3), None);
        assert_eq!(from_bits("1x1", 3), None);
    }

    #[test]
    fn the_colour_coding_of_a_pane_round_trips() {
        let c = ColorCfg { on: true, param: 5, auto: false, min: -0.25, max: 1.5e-3, log: true, palette: 2, by_class: true, hy: true, hy_key: "cube_d2200".into(), hy_label: "White mica D2200".into() };
        let back = color_from_line(&color_to_line(&c)).unwrap();
        assert_eq!((back.on, back.param, back.auto, back.min, back.max, back.log, back.palette, back.by_class), (true, 5, false, -0.25, 1.5e-3, true, 2, true));
        // the hyperspectral choice comes back too (the colour bar needs its label without reading the table)
        assert!(back.hy && back.hy_key == "cube_d2200" && back.hy_label == "White mica D2200" && !back.hyper_on(), "not while it is colouring by class");
        let h = ColorCfg { on: true, by_class: false, ..c.clone() };
        let back = color_from_line(&color_to_line(&h)).unwrap();
        assert!(back.hyper_on() && back.label() == "White mica D2200");
        // a line from before the hyperspectral values (8 fields) reads with them off
        let old = color_from_line("1|5|0|-0.25|0.0015|1|2|0").unwrap();
        assert!(!old.hy && old.hy_key.is_empty() && !old.hyper_on() && old.label() == ams_core::params::PARAM_LABELS[5]);
        assert!(color_from_line("1|2|3").is_none());
        assert!(color_from_line("1|999|1|0|1|0|0|0").is_none());
        assert!(color_from_line("1|1|1|NaN|1|0|0|0").is_none());
        assert!(color_from_line("1|1|1|0|1|0|9|0").is_none());
    }

    #[test]
    fn the_sort_and_filter_of_the_specimen_list_round_trip() {
        for kind in [SortKind::File, SortKind::NumberThenLetter, SortKind::LetterThenNumber, SortKind::Natural, SortKind::Param(7)] {
            for show in [Show::All, Show::Selected, Show::NotSelected, Show::Active, Show::Disregarded] {
                let sv = SpecView { kind, descending: true, bottom_disregarded: false, text: "A1, B1 -ZZ05 = \"x\"|".into(), show, range_on: true, range_param: 9, min_text: "1,5".into(), max_text: "3".into() };
                assert_eq!(sv_from_line(&sv_to_line(&sv)), Some(sv));
            }
        }
        assert!(sv_from_line("junk").is_none());
        assert!(sv_from_line("{\"kind\":\"param999\",\"show\":\"all\"}").is_none());
    }
}
