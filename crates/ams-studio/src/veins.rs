//! Vein planes from the alpha and beta angles measured on the whole core, as the AMS dashboard calculates them
//! (`vein_plane_geographic` in `hyperspectral_dashboard_row_dependancy.py`), and the reader that pulls the veins
//! (`VEIN_PLANES`) and the hole surveys (`BOREHOLE_CONFIGS` in `dashboard_from_dot_ams.ipynb`) out of that code.
//!
//! Alpha / beta are the standard Kenometer angles: beta is measured clockwise looking downhole from the line of the
//! core's BOH mark to the downhole apex of the plane's trace, alpha is the angle between the plane and the core axis.
//! Because the angles were taken on the whole core, the plane needs only the survey of the hole (dip direction and
//! plunge), not the rotation of the cubes (BOH_ROTA).

use crate::hyper_py::{Module, Val};
use std::path::Path;

/// The geographic dip direction and dip (degrees, lower hemisphere) of the plane with core angles `alpha` / `beta`
/// in a hole that goes down towards `hole_dip_dir` with `plunge` below the horizontal.
pub fn plane_geographic(alpha: f64, beta: f64, hole_dip_dir: f64, plunge: f64) -> (f64, f64) {
    // pole of the plane in the core frame (y = BOH line, z = downhole)
    let (d, i) = ((270.0 + beta).to_radians(), alpha.to_radians());
    let v = [i.cos() * d.cos(), i.cos() * d.sin(), i.sin()];
    let (s, dd, p) = ((hole_dip_dir + 90.0).to_radians(), hole_dip_dir.to_radians(), plunge.to_radians());
    let x = [s.sin(), s.cos(), 0.0];
    let z = [p.cos() * dd.sin(), p.cos() * dd.cos(), -p.sin()];
    let y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
    let (mut e, mut n, mut up) = (0.0, 0.0, 0.0);
    for k in 0..3 {
        // columns x, y, z times the core vector: east, north, up
        let c = x[k] * v[0] + y[k] * v[1] + z[k] * v[2];
        match k {
            0 => e = c,
            1 => n = c,
            _ => up = c,
        }
    }
    if up > 0.0 {
        // the pole is taken into the lower hemisphere
        (e, n, up) = (-e, -n, -up);
    }
    let trend = e.atan2(n).to_degrees().rem_euclid(360.0);
    ((trend + 180.0).rem_euclid(360.0), 90.0 - (-up).clamp(0.0, 1.0).asin().to_degrees())
}

/// One vein of the dashboard code.
#[derive(Clone, Debug, PartialEq)]
pub struct DashVein {
    pub name: String,
    pub alpha: f64,
    pub beta: f64,
}

/// What the dashboard code knows about a specimen: its veins and the survey of its hole (dip direction, plunge).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DashSpecimen {
    pub veins: Vec<DashVein>,
    pub survey: Option<(f64, f64)>,
}

/// The parsed dashboard tables, by specimen id (`ER26PB012`).
#[derive(Clone, Debug, Default)]
pub struct Dash {
    veins: Vec<(String, Vec<DashVein>)>,
    surveys: Vec<(String, (f64, f64))>,
}

const MODULE_FILE: &str = crate::hyper::MODULE_FILE;
const NOTEBOOK: &str = "dashboard_from_dot_ams.ipynb";

impl Dash {
    /// Reads the tables from the code folder `dep` (`.../Python_doc/dependancies`).
    pub fn load(dep: &Path) -> Option<Dash> {
        let src = std::fs::read_to_string(dep.join(MODULE_FILE)).ok()?;
        let nb = dep.parent().and_then(|p| std::fs::read_to_string(p.join(NOTEBOOK)).ok()).and_then(|t| notebook_cell(&t, "BOREHOLE_CONFIGS = {"));
        Some(Dash::parse(&src, nb.as_deref().unwrap_or("")))
    }

    /// `module` is the dashboard module's code, `notebook` the notebook cell that holds `BOREHOLE_CONFIGS`.
    pub fn parse(module: &str, notebook: &str) -> Dash {
        let mut d = Dash::default();
        let m = Module::parse(module);
        for (k, v) in m.get("VEIN_PLANES").dict().unwrap_or(&[]) {
            let (Some(id), Some(list)) = (k.str(), v.items()) else { continue };
            let veins: Vec<DashVein> = list.iter().enumerate().filter_map(|(n, e)| Some(DashVein { name: e.get("name").and_then(|s| s.str()).map_or(format!("vein {}", n + 1), |s| s.to_string()), alpha: e.get("alpha")?.num()?, beta: e.get("beta")?.num()? })).collect();
            if !veins.is_empty() {
                d.veins.push((id.to_string(), veins));
            }
        }
        let nb = Module::parse(notebook);
        for (k, v) in nb.get("BOREHOLE_CONFIGS").dict().unwrap_or(&[]) {
            if let (Some(id), Some(dd), Some(p)) = (k.str(), v.get("DIP_DIR").and_then(Val::num), v.get("PLUNGE").and_then(Val::num)) {
                d.surveys.push((id.to_string(), (dd, p)));
            }
        }
        d
    }

    /// The veins and the survey for a specimen id (`ER26PB012`; upper / lower case does not matter).
    pub fn specimen(&self, id: &str) -> DashSpecimen {
        let same = |k: &String| k.eq_ignore_ascii_case(id);
        DashSpecimen { veins: self.veins.iter().find(|(k, _)| same(k)).map(|(_, v)| v.clone()).unwrap_or_default(), survey: self.surveys.iter().find(|(k, _)| same(k)).map(|(_, s)| *s) }
    }

    pub fn is_empty(&self) -> bool {
        self.veins.is_empty() && self.surveys.is_empty()
    }
}

/// The source of the first notebook cell that has a line starting with `marker`.
fn notebook_cell(text: &str, marker: &str) -> Option<String> {
    let nb: serde_json::Value = serde_json::from_str(text).ok()?;
    for cell in nb.get("cells").and_then(|c| c.as_array())? {
        let src: String = match cell.get("source") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|s| s.as_str()).collect(),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => continue,
        };
        if src.lines().any(|l| l.starts_with(marker)) {
            return Some(src);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: (f64, f64), b: (f64, f64)) {
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{a:?} vs {b:?}");
    }

    #[test]
    fn the_planes_agree_with_the_dashboards_python_function() {
        // values printed by vein_plane_geographic in hyperspectral_dashboard_row_dependancy.py
        near(plane_geographic(45.0, 265.0, 316.135, 67.0), (63.44673002169657, 47.547762830849344));
        near(plane_geographic(35.0, 315.0, 305.0, 69.0), (87.18405718301699, 70.85845480209002));
        near(plane_geographic(22.5, 330.0, 305.0, 69.0), (97.41311496001549, 85.95531583832096));
        near(plane_geographic(20.0, 330.0, 305.0, 69.0), (96.96400252083703, 88.41474984181714));
        // alpha 90 = across the core: the plane stands on the hole axis (dip = 90 - plunge, dipping back up the hole's azimuth)
        near(plane_geographic(90.0, 0.0, 305.0, 69.0), (125.0, 21.0));
        // alpha 0, beta 0: along the core, in the vertical plane through the hole's dip direction
        near(plane_geographic(0.0, 0.0, 305.0, 69.0), (305.0, 69.0));
    }

    #[test]
    fn the_veins_and_the_survey_come_from_the_dashboard_code() {
        let module = "VEIN_PLANES = {\n    \"ER26PB012\": [{\"name\": \"vein 1\", \"alpha\": 25, \"beta\": 320, \"zones\": (0, 1)},\n                  {\"name\": \"vein 2 (main)\", \"alpha\": 35, \"beta\": 315, \"zones\": (2, 4, 5)}],\n    # 015 orange: no beta\n    \"ER26PB015\": [{\"name\": \"phyllic vein\", \"alpha\": 20, \"beta\": 330, \"zones\": (0, 1)}],\n}\n";
        let nb = "BOREHOLE_CONFIGS = {\n    \"ER26PB012\": {\n        \"DIP_DIR\": 305.0,\n        \"PLUNGE\": 69.0,\n        \"INTERVALS\": [\n            (0,  32, 45.0),    # comment\n            (33, 49, 65.0)\n        ]\n    },\n}\n";
        let d = Dash::parse(module, nb);
        let s = d.specimen("er26pb012");
        assert_eq!(s.survey, Some((305.0, 69.0)));
        assert_eq!(s.veins, vec![DashVein { name: "vein 1".into(), alpha: 25.0, beta: 320.0 }, DashVein { name: "vein 2 (main)".into(), alpha: 35.0, beta: 315.0 }]);
        assert_eq!(d.specimen("ER26PB015").survey, None);
        assert_eq!(d.specimen("ER26PB015").veins.len(), 1);
        assert!(d.specimen("ER26PB099").veins.is_empty());
    }

    #[test]
    fn the_real_dashboard_code_is_read_when_it_is_there() {
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return };
        let dep = home.join("SynologyDrive/PhD_work/working_files_doc/Python_doc/dependancies");
        let Some(d) = Dash::load(&dep) else { return };
        let s = d.specimen("ER26PB012");
        assert_eq!(s.veins.len(), 3);
        assert_eq!(s.survey, Some((305.0, 69.0)));
    }
}
