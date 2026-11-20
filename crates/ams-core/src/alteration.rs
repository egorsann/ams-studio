//! Alteration intervals of a specimen (the `ALTERATION_INTERVALS` of the AMS dashboard code) and the deviation of a
//! cube's K1 / K3 from the mean of the background cubes.
//!
//! An interval has a type and, per cube letter, the first and last slice that belong to it; single cubes are added
//! (`add`) or taken out (`exclude`). Every cube outside all intervals is background. The deviation is the dashboard's
//! `cm_dir` fill: the axial angle (0-90 deg) between the cube's axis and the axial mean of the same axis over the
//! background cubes whose axis passes its F-tests.

use crate::core::slice_of;
use crate::depjson::{read_section_for, write_section_for};
use crate::tensor::{eig_sym, Mat3};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// 95 % critical values the dashboard uses (`ams_significance_dependancy`): F, and F12 / F23.
pub const F_CRITICAL: f64 = 3.4817;
pub const SUB_F_CRITICAL: f64 = 3.8853;
/// Fewer background cubes with a resolved axis than this: the reference is taken from all cubes with a resolved axis.
pub const MIN_BACKGROUND: usize = 3;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AltInterval {
    pub kind: String,
    /// cube letter -> (first slice, last slice), both included
    pub cubes: BTreeMap<char, (u32, u32)>,
    pub add: Vec<String>,
    pub exclude: Vec<String>,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AltIntervals {
    pub intervals: Vec<AltInterval>,
}

/// Letter and slice of a cube name the way the dashboard reads it (`C24` -> ('C', 24); `A46_CANTED` -> ('A', 46)).
pub fn cube_key(name: &str) -> Option<(char, u32)> {
    let s = slice_of(name)?;
    Some((name.trim().chars().next()?.to_ascii_uppercase(), s))
}

impl AltIntervals {
    /// Does the cube belong to an alteration interval? `None` for a name that is no cube (no letter A-D and slice).
    pub fn altered(&self, name: &str) -> Option<bool> {
        let (l, s) = cube_key(name)?;
        let id = format!("{l}{s}");
        Some(self.intervals.iter().any(|iv| {
            if iv.exclude.iter().any(|e| e.eq_ignore_ascii_case(&id)) {
                return false;
            }
            iv.add.iter().any(|a| a.eq_ignore_ascii_case(&id)) || iv.cubes.get(&l).map_or(false, |&(a, b)| s >= a && s <= b)
        }))
    }

    pub fn to_json(&self, source: &str) -> Value {
        let iv: Vec<Value> = self
            .intervals
            .iter()
            .map(|i| {
                let cubes: serde_json::Map<String, Value> = i.cubes.iter().map(|(l, (a, b))| (l.to_string(), json!([a, b]))).collect();
                json!({ "type": i.kind, "cubes": cubes, "add": i.add, "exclude": i.exclude, "priority": i.priority })
            })
            .collect();
        json!({ "source": source, "intervals": iv })
    }

    pub fn from_json(v: &Value) -> Option<AltIntervals> {
        let arr = v.get("intervals")?.as_array()?;
        let strings = |x: Option<&Value>| -> Vec<String> { x.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
        let intervals = arr
            .iter()
            .map(|i| AltInterval {
                kind: i.get("type").and_then(|t| t.as_str()).unwrap_or("unspecified").to_string(),
                cubes: i
                    .get("cubes")
                    .and_then(|c| c.as_object())
                    .map(|o| o.iter().filter_map(|(k, r)| Some((k.chars().next()?.to_ascii_uppercase(), (r.get(0)?.as_u64()? as u32, r.get(1)?.as_u64()? as u32)))).collect())
                    .unwrap_or_default(),
                add: strings(i.get("add")),
                exclude: strings(i.get("exclude")),
                priority: i.get("priority").and_then(|p| p.as_i64()).unwrap_or(0),
            })
            .collect();
        Some(AltIntervals { intervals })
    }
}

/// The alteration intervals kept for the specimen of `file` (a source file or its `_geo` copy), if any.
pub fn read_alteration(file: &Path) -> Option<AltIntervals> {
    AltIntervals::from_json(&read_section_for(file, "alteration")?)
}

/// Stores (or, with `None`, removes) the alteration intervals of the specimen of `file`; the other sections stay.
pub fn write_alteration(file: &Path, a: Option<&AltIntervals>, source: &str) -> std::io::Result<()> {
    write_section_for(file, "alteration", a.map(|a| a.to_json(source)))
}

// ------------------------------------------------------------------------------------------------ the deviation

fn unit(dec: f64, inc: f64) -> [f64; 3] {
    let (d, i) = (dec.to_radians(), inc.to_radians());
    [i.cos() * d.sin(), i.cos() * d.cos(), i.sin()]
}

/// Mean of axial directions (dec, inc in degrees): the principal eigenvector of their orientation tensor, as a unit
/// (east, north, down) vector. `None` without a finite direction.
pub fn axial_mean(dirs: &[(f64, f64)]) -> Option<[f64; 3]> {
    let mut t: Mat3 = [[0.0; 3]; 3];
    let mut n = 0;
    for &(d, i) in dirs {
        if !(d.is_finite() && i.is_finite()) {
            continue;
        }
        let v = unit(d, i);
        for a in 0..3 {
            for b in 0..3 {
                t[a][b] += v[a] * v[b];
            }
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    Some(eig_sym(&t).1[0])
}

/// Axial angle (0-90 deg) between a direction and the mean `m`.
pub fn axial_angle(dec: f64, inc: f64, m: &[f64; 3]) -> f64 {
    let v = unit(dec, inc);
    (v[0] * m[0] + v[1] * m[1] + v[2] * m[2]).abs().clamp(0.0, 1.0).acos().to_degrees()
}

/// How a reference mean came about, for the note next to the values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefKind {
    /// `n` background cubes with a resolved axis.
    Background(usize),
    /// Too few background cubes (or no intervals): `n` cubes with a resolved axis.
    AllCubes(usize),
    None,
}

/// Deviation (deg) of every direction from the axial mean of the reference cubes: those that are background and resolved
/// (`resolved`), or all resolved ones when fewer than [`MIN_BACKGROUND`] qualify. `background[i]` is `None` for a record
/// that is no cube. NaN for a record without a direction.
pub fn deviations(dirs: &[(f64, f64)], resolved: &[bool], background: &[Option<bool>]) -> (Vec<f64>, RefKind) {
    let finite = |i: usize| dirs[i].0.is_finite() && dirs[i].1.is_finite();
    let bg: Vec<usize> = (0..dirs.len()).filter(|&i| finite(i) && resolved[i] && background[i] == Some(true)).collect();
    let (idx, kind) = if bg.len() >= MIN_BACKGROUND {
        (bg.clone(), RefKind::Background(bg.len()))
    } else {
        let all: Vec<usize> = (0..dirs.len()).filter(|&i| finite(i) && resolved[i]).collect();
        let n = all.len();
        (all, RefKind::AllCubes(n))
    };
    let pick: Vec<(f64, f64)> = idx.iter().map(|&i| dirs[i]).collect();
    // nothing resolved at all: use every direction, like the dashboard
    let m = axial_mean(&pick).or_else(|| axial_mean(dirs));
    let kind = if m.is_none() { RefKind::None } else { kind };
    let out = (0..dirs.len()).map(|i| match (&m, finite(i)) { (Some(m), true) => axial_angle(dirs[i].0, dirs[i].1, m), _ => f64::NAN }).collect();
    (out, kind)
}

// ------------------------------------------------------------------------------------ Python dictionary reader

#[derive(Debug, Clone, PartialEq)]
enum Py {
    Num(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(Py, Py)>),
    Other,
}

struct Parser<'a> {
    s: &'a [u8],
    p: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.p < self.s.len() && (self.s[self.p].is_ascii_whitespace()) {
            self.p += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.p).copied()
    }
    fn value(&mut self) -> Result<Py, String> {
        match self.peek().ok_or("unexpected end")? {
            b'{' => {
                self.p += 1;
                let mut items = vec![];
                loop {
                    if self.peek() == Some(b'}') {
                        self.p += 1;
                        break;
                    }
                    let k = self.value()?;
                    if self.peek() != Some(b':') {
                        return Err(format!("expected ':' at {}", self.p));
                    }
                    self.p += 1;
                    let v = self.value()?;
                    items.push((k, v));
                    if self.peek() == Some(b',') {
                        self.p += 1;
                    }
                }
                Ok(Py::Dict(items))
            }
            c @ (b'[' | b'(') => {
                self.p += 1;
                let close = if c == b'[' { b']' } else { b')' };
                let mut items = vec![];
                loop {
                    if self.peek() == Some(close) {
                        self.p += 1;
                        break;
                    }
                    items.push(self.value()?);
                    if self.peek() == Some(b',') {
                        self.p += 1;
                    }
                }
                Ok(Py::List(items))
            }
            q @ (b'"' | b'\'') => {
                self.p += 1;
                let st = self.p;
                while self.p < self.s.len() && self.s[self.p] != q {
                    self.p += 1;
                }
                let t = String::from_utf8_lossy(&self.s[st..self.p]).to_string();
                self.p += 1;
                Ok(Py::Str(t))
            }
            c if c == b'-' || c.is_ascii_digit() => {
                let st = self.p;
                self.p += 1;
                while self.p < self.s.len() && (self.s[self.p].is_ascii_digit() || matches!(self.s[self.p], b'.' | b'e' | b'E' | b'-')) {
                    self.p += 1;
                }
                std::str::from_utf8(&self.s[st..self.p]).unwrap_or("").parse().map(Py::Num).map_err(|_| format!("bad number at {st}"))
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while self.p < self.s.len() && (self.s[self.p].is_ascii_alphanumeric() || self.s[self.p] == b'_') {
                    self.p += 1;
                }
                Ok(Py::Other)
            }
            c => Err(format!("unexpected '{}' at {}", c as char, self.p)),
        }
    }
}

/// The source with its `#` comments removed (a `#` inside a string stays).
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut quote: Option<char> = None;
    let mut comment = false;
    for ch in src.chars() {
        if comment {
            if ch == '\n' {
                comment = false;
                out.push(ch);
            }
            continue;
        }
        match quote {
            Some(q) => {
                out.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '#' => comment = true,
                '"' | '\'' => {
                    quote = Some(ch);
                    out.push(ch);
                }
                _ => out.push(ch),
            },
        }
    }
    out
}

/// Reads the `ALTERATION_INTERVALS = {...}` dictionary of a Python file (`ams_intervals_dependancy.py`): specimen ID ->
/// intervals. Only the literal is read; nothing is run.
pub fn parse_python_intervals(src: &str) -> Result<BTreeMap<String, AltIntervals>, String> {
    let text = strip_comments(src);
    // the assignment `ALTERATION_INTERVALS = {...}`: the name also appears in text and comments, so every candidate is tried
    // until one reads as a dictionary of lists
    let mut spec = None;
    let mut last_err = "no ALTERATION_INTERVALS = {...} in this file".to_string();
    for (at, _) in text.match_indices("ALTERATION_INTERVALS") {
        let after = text[at + "ALTERATION_INTERVALS".len()..].trim_start();
        let Some(r) = after.strip_prefix('=') else { continue };
        if !r.trim_start().starts_with('{') {
            continue;
        }
        let mut p = Parser { s: text.as_bytes(), p: text.len() - r.len() };
        match p.value() {
            Ok(Py::Dict(d)) if d.iter().any(|(_, v)| matches!(v, Py::List(_))) => {
                spec = Some(d);
                break;
            }
            Ok(_) => last_err = "ALTERATION_INTERVALS is not a dictionary of lists".to_string(),
            Err(e) => last_err = e,
        }
    }
    let spec = spec.ok_or(last_err)?;
    let key = |v: &Py| if let Py::Str(s) = v { Some(s.clone()) } else { None };
    let mut out = BTreeMap::new();
    for (k, v) in spec {
        let (Some(id), Py::List(list)) = (key(&k), v) else { continue };
        let mut ivs = AltIntervals::default();
        for item in list {
            let Py::Dict(fields) = item else { continue };
            let mut iv = AltInterval { kind: "unspecified".into(), ..Default::default() };
            for (fk, fv) in fields {
                match (key(&fk).as_deref(), fv) {
                    (Some("type"), Py::Str(s)) => iv.kind = s,
                    (Some("priority"), Py::Num(n)) => iv.priority = n as i64,
                    (Some("cubes"), Py::Dict(c)) => {
                        for (l, r) in c {
                            if let (Some(l), Py::List(r)) = (key(&l), r) {
                                if let (Some(Py::Num(a)), Some(Py::Num(b)), Some(l)) = (r.get(0), r.get(1), l.chars().next()) {
                                    iv.cubes.insert(l.to_ascii_uppercase(), (*a as u32, *b as u32));
                                }
                            }
                        }
                    }
                    (Some(f @ ("add" | "exclude")), Py::List(l)) => {
                        let names: Vec<String> = l.iter().filter_map(key).map(|s| s.trim().to_uppercase()).collect();
                        if f == "add" { iv.add = names } else { iv.exclude = names }
                    }
                    _ => {}
                }
            }
            ivs.intervals.push(iv);
        }
        out.insert(id, ivs);
    }
    if out.is_empty() {
        return Err("ALTERATION_INTERVALS holds no specimen".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
"""Edit ALTERATION_INTERVALS below, like  ALTERATION_INTERVALS = {x}  in the notes"""
# comment with a brace {
ALTERATION_INTERVALS = {
    "ER26PB002": [
        {"type": "brown/red halo",   "cubes": {"A": (21, 29), "B": (19, 29)}, "add": ["C24"]},
        {"type": "brown/red strong", "cubes": {"A": (23, 28)}, "add": ["C24"], "priority": 1},  # strong
    ],
    "ER26PB004": [
        {"type": "halo", "cubes": {"D": (15, 30)}, "exclude": ["D23"]},
        {"type": "strong", "add": ["A24", "A25"], "priority": 1},
    ],
    # "ER26PB015": [ {"type": "x"} ],
}
VEIN_TYPE = {}
"#;

    #[test]
    fn the_python_dictionary_is_read_without_running_it() {
        let m = parse_python_intervals(SRC).unwrap();
        assert_eq!(m.keys().cloned().collect::<Vec<_>>(), ["ER26PB002", "ER26PB004"]);
        let a = &m["ER26PB002"];
        assert_eq!(a.intervals.len(), 2);
        assert_eq!(a.intervals[0].cubes[&'A'], (21, 29));
        assert_eq!(a.intervals[1].priority, 1);
        assert_eq!(a.intervals[0].add, ["C24"]);
        assert!(parse_python_intervals("x = 1").is_err());
    }

    #[test]
    fn a_cube_is_background_outside_every_interval_and_exclude_and_add_work_per_cube() {
        let m = parse_python_intervals(SRC).unwrap();
        let a = &m["ER26PB002"];
        assert_eq!(a.altered("A21"), Some(true));
        assert_eq!(a.altered("A30"), Some(false));
        assert_eq!(a.altered("C24"), Some(true), "added single cube");
        assert_eq!(a.altered("C23"), Some(false));
        assert_eq!(a.altered("B19_CANTED"), Some(true), "a suffix after the slice does not matter");
        assert_eq!(a.altered("hello"), None);
        let b = &m["ER26PB004"];
        assert_eq!(b.altered("D15"), Some(true));
        assert_eq!(b.altered("D23"), Some(false), "excluded");
        assert_eq!(b.altered("A25"), Some(true));
    }

    #[test]
    fn the_intervals_survive_json() {
        let m = parse_python_intervals(SRC).unwrap();
        let v = m["ER26PB004"].to_json("test");
        assert_eq!(AltIntervals::from_json(&v), Some(m["ER26PB004"].clone()));
    }

    #[test]
    fn deviation_is_the_axial_angle_to_the_mean_of_the_background() {
        // five background cubes with K1 near 90/0 (the axis 90 and 270 count the same), one altered cube at 0/0
        let dirs = [(90.0, 0.0), (270.0, 2.0), (92.0, 1.0), (88.0, 0.0), (270.0, -1.0), (0.0, 0.0)];
        let ok = [true; 6];
        let bg = [Some(true), Some(true), Some(true), Some(true), Some(true), Some(false)];
        let (d, kind) = deviations(&dirs, &ok, &bg);
        assert_eq!(kind, RefKind::Background(5));
        assert!(d[..5].iter().all(|x| *x < 3.0), "{d:?}");
        assert!((d[5] - 90.0).abs() < 3.0, "{d:?}");
        // too few background cubes: all resolved cubes make the reference
        let bg2 = [Some(false), Some(false), Some(true), Some(false), Some(false), Some(false)];
        assert!(matches!(deviations(&dirs, &ok, &bg2).1, RefKind::AllCubes(6)));
        // an unresolved axis is not part of the reference but still gets its angle; no direction gives NaN
        let mut dirs3 = dirs;
        dirs3[1] = (f64::NAN, f64::NAN);
        let ok3 = [true, true, true, true, true, false];
        let (d3, _) = deviations(&dirs3, &ok3, &bg);
        assert!(d3[1].is_nan() && d3[5].is_finite());
    }
}
