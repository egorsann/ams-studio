//! Hyperspectral parameters on the cubes of the core viewer, taken from the AMS dashboard code.
//!
//! The list of parameters, their labels, colour maps and scales are read from `hyperspectral_dashboard_row_dependancy.py`
//! (`CUBE_ROWS`, `CUBE_ROW_DIVERGING`, ..., see `hyper_py.rs`), the order the dashboard prints them in from its notebook
//! (`VISIBLE_ROWS`), and the numbers from the spectral feature table (`ER26PB_spectral_features.csv`) that the dashboard
//! reads, so whatever is added to that code appears in the menu without a change here. Nothing is run: the colours are
//! worked out as `draw_cube_map_row` does it (one fixed scale over the whole table; the median of the shots of a face;
//! grey where a face was measured but has no trough, white where it was not measured), so a cube looks as on the
//! dashboard: the bottom face fills the square and four strips show the faces BK (right), FRONT (left), L (bottom) and
//! R (top).
//!
//! The columns the dashboard derives in `_features()` (position shifts from the pooled median, the water-rich flag, ...) are
//! worked out here by the same rules; a parameter whose column cannot be found or derived is left out of the list.

use crate::hyper_py::{Module, Val};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MODULE_FILE: &str = "hyperspectral_dashboard_row_dependancy.py";
const EXTRACT_FILE: &str = "hyperspectral_extract_asd_dependancy.py";
const NOTEBOOK: &str = "dashboard_from_dot_ams.ipynb";

/// Colour of a face on a cube.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Face {
    /// The face was not measured (white).
    NotMeasured,
    /// Measured, but no feature could be placed (grey).
    NoTrough,
    /// The value of the face (median of its shots) and its colour.
    Value(f64, [u8; 3]),
}

/// The five faces of a cube: bottom (the fill), BK (right strip), FRONT (left strip), L (bottom strip), R (top strip).
pub type Faces = [Face; 5];

#[derive(Clone, Debug)]
enum Cmap {
    Named(String, bool),
    List(Vec<[u8; 3]>),
}

impl Cmap {
    fn eval(&self, t: f64) -> [u8; 3] {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Cmap::List(v) => {
                if v.len() == 1 {
                    return v[0];
                }
                let x = t * (v.len() - 1) as f64;
                let (i, f) = ((x.floor() as usize).min(v.len() - 2), x - x.floor().min((v.len() - 2) as f64));
                let f = if x >= (v.len() - 1) as f64 { 1.0 } else { f };
                let (a, b) = (v[i], v[i + 1]);
                [0, 1, 2].map(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8)
            }
            Cmap::Named(n, rev) => {
                let g = match n.as_str() {
                    "PuOr" => colorous::PURPLE_ORANGE,
                    "BrBG" => colorous::BROWN_GREEN,
                    "PiYG" => colorous::PINK_GREEN,
                    "RdYlBu" => colorous::RED_YELLOW_BLUE,
                    "RdBu" => colorous::RED_BLUE,
                    "RdYlGn" => colorous::RED_YELLOW_GREEN,
                    "PRGn" => colorous::PURPLE_GREEN,
                    "Spectral" => colorous::SPECTRAL,
                    "plasma" => colorous::PLASMA,
                    "magma" => colorous::MAGMA,
                    "inferno" => colorous::INFERNO,
                    "cividis" => colorous::CIVIDIS,
                    "turbo" => colorous::TURBO,
                    "Blues" => colorous::BLUES,
                    "Greens" => colorous::GREENS,
                    "Greys" => colorous::GREYS,
                    "Oranges" => colorous::ORANGES,
                    "Purples" => colorous::PURPLES,
                    "Reds" => colorous::REDS,
                    "YlOrBr" => colorous::YELLOW_ORANGE_BROWN,
                    "PuBu" => colorous::PURPLE_BLUE,
                    _ => colorous::VIRIDIS,
                };
                // `colorous` runs the same way as matplotlib for every map used here except PuOr (purple to orange there,
                // orange to purple in matplotlib)
                let flip = *rev ^ (n == "PuOr");
                let c = g.eval_continuous(if flip { 1.0 - t } else { t });
                [c.r, c.g, c.b]
            }
        }
    }

    fn from_name(name: &str) -> Cmap {
        match name.strip_suffix("_r") {
            Some(n) => Cmap::Named(n.to_string(), true),
            None => Cmap::Named(name.to_string(), false),
        }
    }
}

fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6).then(|| u32::from_str_radix(s, 16).ok()).flatten().map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A matplotlib colour as the dashboard writes it: `#rrggbb`, a grey level `"0.80"`, or a name (`white`, `black`).
fn colour(v: &Val, default: [u8; 3]) -> [u8; 3] {
    match v.str() {
        Some(s) => hex(s)
            .or_else(|| s.parse::<f64>().ok().map(|g| [(g.clamp(0.0, 1.0) * 255.0).round() as u8; 3]))
            .or_else(|| match s {
                "white" => Some([255; 3]),
                "black" => Some([0; 3]),
                _ => None,
            })
            .unwrap_or(default),
        None => default,
    }
}

#[derive(Clone, Debug)]
enum Half {
    Fixed(f64),
    Quantile(f64),
}

#[derive(Clone, Debug)]
enum Scale {
    /// One fixed scale from 0 (or the 2nd percentile) to the 98th percentile of the table.
    Seq { col: String, ok: Option<String> },
    Div { cmap: Cmap, half: Half },
}

/// One parameter of the menu.
#[derive(Clone, Debug)]
pub struct RowDef {
    pub key: String,
    col: String,
    ok: Option<String>,
    /// The label of the axis on the dashboard (`White mica\nD2200`) and its short form, as one line.
    pub label: String,
    pub short: String,
    /// The label of the colour bar.
    pub unit: String,
    /// The dashboard prints this parameter (it is in the notebook's `VISIBLE_ROWS`).
    pub in_dashboard: bool,
    scale: Scale,
    /// Colour scale from `vmin` to `vmax`.
    pub vmin: f64,
    pub vmax: f64,
    cmap: Cmap,
}

impl RowDef {
    pub fn colour_at(&self, t: f64) -> [u8; 3] {
        self.cmap.eval(t)
    }
    fn paint(&self, v: f64) -> [u8; 3] {
        self.cmap.eval((v - self.vmin) / (self.vmax - self.vmin).max(1e-12))
    }
}

/// How the cubes are drawn (the constants of the dashboard code).
#[derive(Clone, Debug)]
pub struct Look {
    /// Width of a side strip as a fraction of the cube.
    pub edge: f32,
    /// Size of a D cube relative to the others.
    pub d_scale: f32,
    pub no_trough: [u8; 3],
    pub not_measured: [u8; 3],
    pub outline: [u8; 3],
}

/// The spectral feature table with the columns the dashboard derives.
struct Table {
    cols: HashMap<String, usize>,
    rows: Vec<Vec<String>>,
    derived: HashMap<String, Vec<f64>>,
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    for c in line.chars() {
        match c {
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn cell_num(s: &str) -> f64 {
    let t = s.trim();
    match t {
        "" => f64::NAN,
        "True" | "true" | "TRUE" => 1.0,
        "False" | "false" | "FALSE" => 0.0,
        _ => t.parse().unwrap_or(f64::NAN),
    }
}

/// pandas' default quantile (linear between the two nearest values) of the finite ones.
pub fn quantile(v: &[f64], q: f64) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return f64::NAN;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = q.clamp(0.0, 1.0) * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

/// `np.round(x, d)`: halves go to the even neighbour.
fn round_to(x: f64, d: i32) -> f64 {
    let k = 10f64.powi(d);
    (x * k).round_ties_even() / k
}

impl Table {
    fn parse(text: &str) -> Result<Table, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let head = split_csv(lines.next().ok_or("The feature table is empty.")?);
        let cols: HashMap<String, usize> = head.iter().enumerate().map(|(i, h)| (h.trim().to_string(), i)).collect();
        for need in ["specimen", "cube", "side"] {
            if !cols.contains_key(need) {
                return Err(format!("The feature table has no column \u{201c}{need}\u{201d}."));
            }
        }
        let rows: Vec<Vec<String>> = lines.map(split_csv).collect();
        Ok(Table { cols, rows, derived: HashMap::new() })
    }

    fn has(&self, name: &str) -> bool {
        self.derived.contains_key(name) || self.cols.contains_key(name)
    }

    /// A column as numbers (booleans as 1 / 0, empty as NaN).
    fn num(&self, name: &str) -> Option<Vec<f64>> {
        if let Some(d) = self.derived.get(name) {
            return Some(d.clone());
        }
        let i = *self.cols.get(name)?;
        Some(self.rows.iter().map(|r| r.get(i).map_or(f64::NAN, |c| cell_num(c))).collect())
    }

    fn flag(&self, name: &str) -> Option<Vec<bool>> {
        self.num(name).map(|v| v.into_iter().map(|x| x.is_finite() && x != 0.0).collect())
    }

    fn text(&self, name: &str) -> Vec<&str> {
        let i = self.cols[name];
        self.rows.iter().map(|r| r.get(i).map_or("", |c| c.trim())).collect()
    }

    fn set_flag(&mut self, name: &str, v: Vec<bool>) {
        self.derived.insert(name.to_string(), v.into_iter().map(|b| b as u8 as f64).collect());
    }

    /// The columns `_features()` of the dashboard adds, by the same rules.
    fn derive(&mut self, c: &Consts) {
        let n = self.rows.len();
        let all = |b: bool| vec![b; n];
        let nan = || vec![f64::NAN; n];
        // epidote: only troughs centred in the epidote window
        if let (Some(ok), Some(w)) = (self.flag("1550_ok"), self.num("W1550")) {
            self.set_flag("epidote_ok", (0..n).map(|i| ok[i] && w[i] >= c.epidote.0 && w[i] <= c.epidote.1).collect());
        }
        // Al-OH position: where the 2200 trough is deep enough, shift from the pooled median
        if let (Some(ok), Some(d), Some(w)) = (self.flag("2200_ok"), self.num("D2200"), self.num("W2200")) {
            let okv: Vec<bool> = (0..n).map(|i| ok[i] && d[i] >= c.ism_min).collect();
            let sel: Vec<f64> = (0..n).filter(|&i| okv[i]).map(|i| w[i]).collect();
            let r = round_to(quantile(&sel, 0.5), 1);
            self.set_flag("W2200_ok", okv);
            self.derived.insert("W2200_shift".into(), w.iter().map(|x| x - r).collect());
        }
        // chlorite Fe-OH position on the local continuum
        if let (Some(ok), Some(w)) = (self.flag("2250L_ok"), self.num("W2250L")) {
            let sel: Vec<f64> = (0..n).filter(|&i| ok[i]).map(|i| w[i]).collect();
            let r = round_to(quantile(&sel, 0.5), 1);
            self.derived.insert("W2250L_shift".into(), w.iter().map(|x| x - r).collect());
        } else if self.has("D2250L") || self.has("W2250") {
            // an old table without the local continuum: the row shows as not measured
            self.derived.insert("W2250L_shift".into(), nan());
            self.set_flag("2250L_ok", all(false));
        }
        // Fe3+ band position from a fixed reference
        if let Some(w) = self.num("W900") {
            self.derived.insert("W900_shift".into(), w.iter().map(|x| x - c.w900_ref).collect());
        }
        // zeolite water bands: positions only on zeolite-like faces, shift from the median of those
        if let Some(z) = self.flag("zeolite_flag") {
            for band in ["1400", "1900"] {
                let (okc, wc) = (format!("{band}_ok"), format!("W{band}"));
                let (Some(ok), Some(w)) = (self.flag(&okc), self.num(&wc)) else { continue };
                let okz: Vec<bool> = (0..n).map(|i| z[i] && ok[i]).collect();
                let sel: Vec<f64> = (0..n).filter(|&i| okz[i]).map(|i| w[i]).collect();
                let r = if sel.is_empty() { f64::NAN } else { round_to(quantile(&sel, 0.5), 1) };
                self.set_flag(&format!("zeo{band}_ok"), okz);
                self.derived.insert(format!("W{band}z_shift"), w.iter().map(|x| x - r).collect());
            }
            // ISM without the water-rich shots
            if let Some(ism) = self.flag("ISM_ok") {
                let long1400: Vec<bool> = match (self.flag("1400_ok"), self.num("W1400")) {
                    (Some(ok), Some(w)) => (0..n).map(|i| ok[i] && w[i] >= c.water_rich).collect(),
                    _ => all(false),
                };
                let wr: Vec<bool> = (0..n).map(|i| z[i] || long1400[i]).collect();
                self.set_flag("ISM_dry_ok", (0..n).map(|i| ism[i] && !wr[i]).collect());
                self.set_flag("water_rich", wr);
            }
        }
    }
}

/// Numbers of the dashboard code the derived columns need.
struct Consts {
    epidote: (f64, f64),
    ism_min: f64,
    w900_ref: f64,
    water_rich: f64,
}

/// The parameters, the feature table and how to draw.
pub struct Hyper {
    pub rows: Vec<RowDef>,
    pub look: Look,
    table: Table,
    /// Where the code and the table were found (for the menu's tooltip).
    pub source: PathBuf,
    pub table_path: PathBuf,
}

/// The folder `dependancies` of the dashboard code, found from a data path: `working_files_doc/Python_doc/dependancies`
/// at the same place relative to the data on every computer that shares the folder (`AMS_STUDIO_HYPER_DIR` overrides).
pub fn locate(from: &Path) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("AMS_STUDIO_HYPER_DIR").map(PathBuf::from).filter(|d| d.join(MODULE_FILE).is_file()) {
        return Some(d);
    }
    for a in from.ancestors() {
        for rel in ["working_files_doc/Python_doc/dependancies", "PhD_work/working_files_doc/Python_doc/dependancies", "Python_doc/dependancies"] {
            let d = a.join(rel);
            if d.join(MODULE_FILE).is_file() {
                return Some(d);
            }
        }
    }
    None
}

fn find_dir(root: &Path, name: &str, depth: usize, budget: &mut usize) -> Option<PathBuf> {
    let rd = std::fs::read_dir(root).ok()?;
    let mut subs = vec![];
    for e in rd.filter_map(|e| e.ok()) {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let p = e.path();
        if p.is_dir() {
            if e.file_name().to_string_lossy() == name {
                return Some(p);
            }
            subs.push(p);
        }
    }
    if depth == 0 {
        return None;
    }
    subs.into_iter().find_map(|p| find_dir(&p, name, depth - 1, budget))
}

fn first_string_after(src: &str, name: &str, pat: &str) -> Option<String> {
    let at = src.find(&format!("\n{name}"))?;
    let line_end = src[at + 1..].find("\n\n").map_or(src.len(), |e| at + 1 + e);
    let seg = &src[at..line_end];
    let q = seg.find(pat)?;
    let rest = &seg[q + pat.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// The names in the notebook's `VISIBLE_ROWS` (the parameters the dashboard prints, in its order).
fn visible_rows(dep: &Path) -> Vec<String> {
    let Some(py) = dep.parent() else { return vec![] };
    let Ok(text) = std::fs::read_to_string(py.join(NOTEBOOK)) else { return vec![] };
    let Ok(nb) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    for cell in nb.get("cells").and_then(|c| c.as_array()).into_iter().flatten() {
        let src: String = match cell.get("source") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|s| s.as_str()).collect(),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => continue,
        };
        if src.lines().any(|l| l.starts_with("VISIBLE_ROWS = [")) {
            let m = Module::parse(&src);
            if let Some(items) = m.get("VISIBLE_ROWS").items() {
                return items.iter().filter_map(|v| v.str().map(|s| s.to_string())).collect();
            }
        }
    }
    vec![]
}

impl Hyper {
    /// Reads the code in `dep` and the feature table it points at.
    pub fn load(dep: &Path) -> Result<Hyper, String> {
        let src = std::fs::read_to_string(dep.join(MODULE_FILE)).map_err(|e| format!("{}: {e}", dep.join(MODULE_FILE).display()))?;
        let extract = std::fs::read_to_string(dep.join(EXTRACT_FILE)).unwrap_or_default();
        let csv_name = first_string_after(&src, "FEATURES_CSV", "\"").filter(|s| s.ends_with(".csv")).unwrap_or_else(|| "ER26PB_spectral_features.csv".into());
        let out_dir = first_string_after(&extract, "OUT_DIR", "SPECTRAL_ROOT, \"").unwrap_or_else(|| "core_samples_SPECTRAL_extracted".into());
        let mut table_path = None;
        for a in dep.ancestors().skip(1).take(5) {
            let mut budget = 30_000;
            if let Some(d) = find_dir(a, &out_dir, 6, &mut budget) {
                let f = d.join(&csv_name);
                if f.is_file() {
                    table_path = Some(f);
                    break;
                }
            }
        }
        let table_path = table_path.ok_or_else(|| format!("The spectral feature table {csv_name} was not found next to the dashboard code."))?;
        let text = std::fs::read_to_string(&table_path).map_err(|e| format!("{}: {e}", table_path.display()))?;
        let mut h = Hyper::from_parts(&src, &extract, &text, &visible_rows(dep))?;
        h.source = dep.to_path_buf();
        h.table_path = table_path;
        Ok(h)
    }

    /// The parameters and scales from the code (`src`, and `extract` for the constants of the extraction module) and the
    /// feature table (`csv`); `visible` the parameters the dashboard prints.
    pub fn from_parts(src: &str, extract: &str, csv: &str, visible: &[String]) -> Result<Hyper, String> {
        let m = Module::parse(src);
        let me = Module::parse(extract);
        let mut table = Table::parse(csv)?;
        let num = |v: Val, d: f64| v.num().unwrap_or(d);
        let pair = |v: Val, d: (f64, f64)| match v.items() {
            Some([a, b]) => (a.num().unwrap_or(d.0), b.num().unwrap_or(d.1)),
            _ => d,
        };
        let consts = Consts { epidote: pair(m.get("EPIDOTE_W_RANGE"), (1545.0, 1565.0)), ism_min: num(me.get("ISM_MIN_D2200"), 0.05), w900_ref: num(m.get("W900_REF"), 900.0), water_rich: num(m.get("WATER_RICH_W1400"), 1413.0) };
        table.derive(&consts);
        let look = Look {
            edge: num(m.get("EDGE_FRAC"), 0.24) as f32,
            d_scale: num(m.get("D_CUBE_SCALE"), 0.6) as f32,
            no_trough: colour(&m.get("NO_TROUGH_COLOR"), [204; 3]),
            not_measured: colour(&m.get("NOT_MEASURED_COLOR"), [255; 3]),
            outline: colour(&m.get("CUBE_OUTLINE"), [89; 3]),
        };
        let cube_rows = m.get("CUBE_ROWS");
        let defs = cube_rows.dict().ok_or("CUBE_ROWS was not found in the dashboard code.")?;
        let div = m.get("CUBE_ROW_DIVERGING");
        let div_q = m.get("CUBE_ROW_DIVERGING_Q");
        let div_half = m.get("CUBE_ROW_DIVERGING_HALF");
        let norm_from = m.get("CUBE_ROW_NORM_FROM");
        let shorts = m.get("CUBE_ROW_SHORT_LABELS");
        let seq_name = m.get("CUBE_CMAP").str().unwrap_or("viridis").to_string();
        let one_line = |s: &str| s.replace('\n', " ");
        let mut rows: Vec<RowDef> = vec![];
        for (k, v) in defs {
            let (Some(key), Some(t)) = (k.str(), v.items()) else { continue };
            let (Some(col), Some(label)) = (t.first().and_then(|c| c.str()), t.get(2).and_then(|c| c.str())) else { continue };
            let ok = t.get(1).and_then(|c| c.str()).map(|s| s.to_string());
            let unit = t.get(3).and_then(|c| c.str()).unwrap_or("").to_string();
            // the column (and its ok flag) must exist in the table
            if !table.has(col) || ok.as_deref().is_some_and(|o| !table.has(o)) {
                continue;
            }
            let scale = match div.get(key) {
                Some(cm) => {
                    let cmap = match cm {
                        Val::Str(n) => Cmap::from_name(n),
                        other => Cmap::List(other.items().unwrap_or(&[]).iter().filter_map(|c| c.str().and_then(hex)).collect()),
                    };
                    let half = match div_half.get(key).and_then(|h| h.num()) {
                        Some(h) => Half::Fixed(h),
                        None => Half::Quantile(div_q.get(key).and_then(|q| q.num()).unwrap_or(0.98)),
                    };
                    Scale::Div { cmap, half }
                }
                None => {
                    // a row that borrows another row's scale (`CUBE_ROW_NORM_FROM`)
                    let src_key = norm_from.get(key).and_then(|s| s.str()).unwrap_or(key);
                    let (c, o) = defs.iter().find(|(k2, _)| k2.str() == Some(src_key)).and_then(|(_, v2)| v2.items()).map(|t2| (t2.first().and_then(|c| c.str()).unwrap_or(col).to_string(), t2.get(1).and_then(|c| c.str()).map(|s| s.to_string()))).unwrap_or((col.to_string(), ok.clone()));
                    Scale::Seq { col: c, ok: o }
                }
            };
            let short = shorts.get(key).and_then(|s| s.str()).map(one_line).unwrap_or_else(|| one_line(label));
            rows.push(RowDef { key: key.to_string(), col: col.to_string(), ok, label: one_line(label), short, unit, in_dashboard: visible.iter().any(|v| v == key), scale, vmin: 0.0, vmax: 1.0, cmap: Cmap::from_name(&seq_name) });
        }
        if rows.is_empty() {
            return Err("No parameter of CUBE_ROWS matches a column of the feature table.".into());
        }
        // the ones the dashboard prints first, in its order, then the others as the code lists them
        let pos = |r: &RowDef| visible.iter().position(|v| *v == r.key).unwrap_or(usize::MAX);
        rows.sort_by_key(pos);
        for r in rows.iter_mut() {
            match r.scale.clone() {
                Scale::Seq { col, ok } => {
                    let v = table.num(&col).unwrap_or_default();
                    let sel: Vec<f64> = match ok.as_deref().and_then(|o| table.flag(o)) {
                        Some(f) => v.iter().zip(&f).filter(|(_, f)| **f).map(|(x, _)| *x).collect(),
                        None => v,
                    };
                    r.vmax = round_to(quantile(&sel, 0.98), 3);
                    r.vmin = if ok.is_some() { 0.0 } else { round_to(quantile(&sel, 0.02), 3) };
                }
                Scale::Div { cmap, half } => {
                    let h = match half {
                        Half::Fixed(h) => h,
                        Half::Quantile(q) => {
                            let v = table.num(&r.col).unwrap_or_default();
                            let abs: Vec<f64> = match r.ok.as_deref().and_then(|o| table.flag(o)) {
                                Some(f) => v.iter().zip(&f).filter(|(_, f)| **f).map(|(x, _)| x.abs()).collect(),
                                None => v.iter().map(|x| x.abs()).collect(),
                            };
                            round_to(quantile(&abs, q), 1)
                        }
                    };
                    r.vmin = -h;
                    r.vmax = h;
                    r.cmap = cmap;
                }
            }
            if !(r.vmax > r.vmin) {
                r.vmax = r.vmin + 1.0;
            }
        }
        Ok(Hyper { rows, look, table, source: PathBuf::new(), table_path: PathBuf::new() })
    }

    /// Whether the table has rows for a specimen.
    pub fn has_specimen(&self, id: &str) -> bool {
        self.table.text("specimen").iter().any(|s| *s == id)
    }

    /// The faces of every cube of a specimen for one parameter: the median of the shots of a face, coloured on the
    /// parameter's scale.
    pub fn faces(&self, specimen: &str, row: usize) -> HashMap<(u32, char), Faces> {
        let mut out: HashMap<(u32, char), Faces> = HashMap::new();
        let Some(r) = self.rows.get(row) else { return out };
        let t = &self.table;
        let (spec, cube, side) = (t.text("specimen"), t.text("cube"), t.text("side"));
        let vals = t.num(&r.col).unwrap_or_default();
        let ok = r.ok.as_deref().and_then(|o| t.flag(o));
        let mut groups: HashMap<(u32, char, usize), Vec<f64>> = HashMap::new();
        for i in 0..spec.len() {
            if spec[i] != specimen {
                continue;
            }
            let mut ch = cube[i].chars();
            let Some(letter) = ch.next() else { continue };
            let Ok(slice) = ch.as_str().parse::<u32>() else { continue };
            let Some(face) = ["BTM", "BK", "FRONT", "L", "R"].iter().position(|f| *f == side[i]) else { continue };
            let v = match &ok {
                Some(f) if !f[i] => f64::NAN,
                _ => vals.get(i).copied().unwrap_or(f64::NAN),
            };
            groups.entry((slice, letter, face)).or_default().push(v);
        }
        for ((slice, letter, face), v) in groups {
            let med = quantile(&v, 0.5);
            let f = if med.is_finite() { Face::Value(med, r.paint(med)) } else { Face::NoTrough };
            out.entry((slice, letter)).or_insert([Face::NotMeasured; 5])[face] = f;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r##"
FEATURES_CSV = os.path.join(_SPEC_OUT, "ER26PB_spectral_features.csv")
FE3_METRIC = "D900"
CUBE_ROWS = {
    "cube_d2200": ("D2200", "2200_ok", "White mica\nD2200", "depth"),
    "cube_w2200": ("W2200_shift", "W2200_ok", "Al-OH pos.\nW2200 shift", "Δnm"),
    "cube_w900": ("W900_shift", "900_ok", "Fe3+ pos.\nW900 shift", "Δnm"),
    "cube_ism": ("ISM", "ISM_ok", "ISM", "ISM"),
    "cube_ism_dry": ("ISM", "ISM_dry_ok", "ISM, water-\nrich removed", "ISM"),
    "cube_missing": ("NOPE", None, "x", "y"),
}
CUBE_ROW_NORM_FROM = {"cube_ism_dry": "cube_ism"}
CUBE_ROW_SHORT_LABELS = {"cube_d2200": "WM\nD2200"}
CUBE_ROW_DIVERGING = {"cube_w2200": "PuOr_r", "cube_w900": ["#7f0000", "#f7f7f7", "#7a4a00"]}
W900_REF, W900_HALF = 900.0, 30.0
CUBE_ROW_DIVERGING_HALF = {"cube_w900": W900_HALF}
EDGE_FRAC = 0.24
D_CUBE_SCALE = 0.6
NO_TROUGH_COLOR, NOT_MEASURED_COLOR, CUBE_OUTLINE = "0.80", "white", "0.35"
WATER_RICH_W1400 = 1413.0
"##;

    const CSV: &str = "specimen,cube,side,D2200,2200_ok,W2200,W900,900_ok,ISM,ISM_ok,zeolite_flag,1400_ok,W1400\n\
        ER1,A1,BTM,0.20,True,2200.0,880.0,True,2.0,True,False,True,1410.0\n\
        ER1,A1,BTM,0.30,True,2204.0,890.0,True,3.0,True,False,True,1410.0\n\
        ER1,A1,BK,0.10,True,2196.0,,False,4.0,True,False,True,1415.0\n\
        ER1,A1,L,0.01,False,2199.0,900.0,True,5.0,True,True,True,1410.0\n\
        ER1,B1,BTM,0.40,True,2201.0,930.0,True,6.0,True,False,True,1410.0\n\
        ER2,A1,BTM,0.90,True,2200.0,900.0,True,1.0,True,False,True,1410.0\n";

    fn load() -> Hyper {
        Hyper::from_parts(SRC, "ISM_MIN_D2200 = 0.05\n", CSV, &["cube_w2200".into(), "cube_d2200".into()]).unwrap()
    }

    #[test]
    fn the_parameters_come_from_the_code_in_the_dashboard_order() {
        let h = load();
        let keys: Vec<&str> = h.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["cube_w2200", "cube_d2200", "cube_w900", "cube_ism", "cube_ism_dry"], "visible ones first, then the code's order; the row with no column is left out");
        assert!(h.rows[0].in_dashboard && !h.rows[2].in_dashboard);
        assert_eq!(h.rows[1].label, "White mica D2200");
        assert_eq!(h.rows[1].short, "WM D2200");
        assert_eq!(h.rows[0].unit, "Δnm");
        assert!((h.look.edge - 0.24).abs() < 1e-6 && (h.look.d_scale - 0.6).abs() < 1e-6);
        assert_eq!(h.look.no_trough, [204, 204, 204]);
        assert_eq!(h.look.outline, [89, 89, 89]);
    }

    #[test]
    fn scales_follow_the_dashboard_rules() {
        let h = load();
        let by = |k: &str| h.rows.iter().find(|r| r.key == k).unwrap();
        // depth: 0 .. 98th percentile of the valid depths (0.2, 0.3, 0.1, 0.4, 0.9)
        let d = by("cube_d2200");
        assert_eq!(d.vmin, 0.0);
        assert!((d.vmax - round_to(quantile(&[0.2, 0.3, 0.1, 0.4, 0.9], 0.98), 3)).abs() < 1e-9);
        // a fixed half range for the Fe3+ shift
        let w = by("cube_w900");
        assert_eq!((w.vmin, w.vmax), (-30.0, 30.0));
        // the position shift is a symmetric scale about 0 (a quantile of |shift| of the placed troughs)
        let s = by("cube_w2200");
        assert!(s.vmin < 0.0 && (s.vmin + s.vmax).abs() < 1e-9);
        // the dry ISM borrows the scale of ISM
        assert_eq!((by("cube_ism").vmin, by("cube_ism").vmax), (by("cube_ism_dry").vmin, by("cube_ism_dry").vmax));
    }

    #[test]
    fn faces_are_medians_grey_where_there_is_no_trough_and_white_where_not_measured() {
        let h = load();
        let row = h.rows.iter().position(|r| r.key == "cube_d2200").unwrap();
        let f = h.faces("ER1", row);
        let a1 = f[&(1, 'A')];
        // the bottom face: median of 0.20 and 0.30
        assert!(matches!(a1[0], Face::Value(v, _) if (v - 0.25).abs() < 1e-9));
        assert!(matches!(a1[1], Face::Value(v, _) if (v - 0.10).abs() < 1e-9), "the right strip");
        assert_eq!(a1[2], Face::NotMeasured, "FRONT was not measured");
        assert_eq!(a1[3], Face::NoTrough, "L was measured but 2200_ok is False");
        assert_eq!(a1[4], Face::NotMeasured);
        assert!(f.contains_key(&(1, 'B')) && !f.contains_key(&(2, 'A')));
        assert!(h.has_specimen("ER2") && !h.has_specimen("ER9"));
        // another specimen's cubes do not mix in
        assert!(matches!(h.faces("ER2", row)[&(1, 'A')][0], Face::Value(v, _) if (v - 0.9).abs() < 1e-9));
    }

    #[test]
    fn derived_columns_follow_the_dashboard() {
        let h = load();
        // W2200_shift is relative to the pooled median of W2200 where the 2200 trough is deep enough
        let sh = h.table.num("W2200_shift").unwrap();
        let placed = [2200.0, 2204.0, 2196.0, 2201.0, 2200.0];
        let r = round_to(quantile(&placed, 0.5), 1);
        assert!((sh[1] - (2204.0 - r)).abs() < 1e-9);
        // the dry flag drops the zeolite-like face and the long 1400 band (>= 1413)
        let dry = h.table.flag("ISM_dry_ok").unwrap();
        assert_eq!(dry, [true, true, false, false, true, true]);
        // W900_shift is from the fixed reference
        assert_eq!(h.table.num("W900_shift").unwrap()[0], -20.0);
    }

    #[test]
    fn colour_maps_run_the_way_matplotlib_does() {
        // PuOr_r: low = purple, high = orange (the dashboard's comment: purple = shorter, orange = longer)
        let c = Cmap::from_name("PuOr_r");
        let (lo, hi) = (c.eval(0.0), c.eval(1.0));
        assert!(lo[2] > lo[0], "purple at the low end: {lo:?}");
        assert!(hi[0] > hi[2], "orange at the high end: {hi:?}");
        // viridis: dark purple to yellow
        let v = Cmap::from_name("viridis");
        assert!(v.eval(0.0)[2] > v.eval(0.0)[0] && v.eval(1.0)[0] > 200);
        // a colour list goes through its colours
        let l = Cmap::List(vec![[0, 0, 0], [255, 255, 255]]);
        assert_eq!(l.eval(0.5), [128, 128, 128]);
        assert_eq!(l.eval(2.0), [255, 255, 255]);
        assert_eq!(hex("#7f0000"), Some([127, 0, 0]));
        assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0], 0.5), 2.5);
        assert_eq!(round_to(2.5, 0), 2.0);
    }

    /// `AMS_STUDIO_TEST_HYPER=<any path of the data> cargo test a_real_dashboard -- --ignored --nocapture` prints what the real code gives.
    #[test]
    #[ignore]
    fn a_real_dashboard_can_be_read() {
        let Some(p) = std::env::var_os("AMS_STUDIO_TEST_HYPER") else { return };
        let dep = locate(Path::new(&p)).expect("dashboard code not found");
        let h = Hyper::load(&dep).unwrap();
        println!("{} / {}", h.source.display(), h.table_path.display());
        for (i, r) in h.rows.iter().enumerate() {
            let f = h.faces("ER26PB012", i);
            println!("{:<16} {:<4} {:<30} scale {:.3} .. {:.3} ({}) cubes {}", r.key, if r.in_dashboard { "dash" } else { "" }, r.label, r.vmin, r.vmax, r.unit, f.len());
        }
    }
}
