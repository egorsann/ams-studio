//! Where a specimen was taken: the `location` section of the specimen's dependency file
//! (`<specimen folder>_geo/<specimen folder>_geo_dependancy.json`, see `ams_core::depjson`), kept with the data like
//! the core viewer link and the stereonet planes. The `.ams` record has no field for a position (its free bytes are
//! used by the Owens block, and a position belongs to the specimen, not to each record), and the source files are
//! never written.
//!
//! ```json
//! "location": { "x": 412345.678, "y": 4851234.567, "crs": { "type": "utm", "zone": 33, "north": true },
//!               "lon": 13.91019, "lat": 43.80898, "source": "typed",
//!               "depth_m": 152.3, "hole": { "dip_direction": 316.1, "plunge": 67.0 }, "elevation_m": 412.0 }
//! ```
//!
//! `source` is `typed`, or `datasheet` with the name of the row and the file it was taken from. For a specimen of drill
//! core, `depth_m` is the depth along the hole the specimen comes from and `hole` the direction the hole was drilled
//! (the azimuth it goes down towards and the angle below the horizontal); specimens of one collar share x and y and
//! differ in depth. Both are optional. Anything else (a block sample, the Owens method) has no depth but an
//! `elevation_m`, metres above sea level.

use super::gis_data::{crs_from_json, crs_json};
use ams_core::depjson;
use ams_core::gis::{self, Crs};
use serde_json::{json, Value};
use std::path::Path;

/// The direction a hole was drilled: the azimuth it goes down towards and the plunge below the horizontal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub dip_dir: f64,
    pub plunge: f64,
}

impl Hole {
    /// `None` unless the azimuth is 0 to 360 and the plunge 0 to 90 degrees.
    pub fn new(dip_dir: f64, plunge: f64) -> Option<Hole> {
        ((0.0..=360.0).contains(&dip_dir) && (0.0..=90.0).contains(&plunge)).then_some(Hole { dip_dir: dip_dir.rem_euclid(360.0), plunge })
    }
}

/// A depth along the hole in metres: not negative, not absurd.
pub fn valid_depth(d: f64) -> bool {
    d.is_finite() && (0.0..=20_000.0).contains(&d)
}

/// An elevation above sea level in metres (below it when negative): a number within what exists on the earth.
pub fn valid_elevation(e: f64) -> bool {
    e.is_finite() && (-1_000.0..=9_000.0).contains(&e)
}

/// `152 m asl`, `-12.5 m asl`.
pub fn elevation_text(e: f64) -> String {
    if (e - e.round()).abs() < 0.05 {
        format!("{:.0} m asl", e)
    } else {
        format!("{:.1} m asl", e)
    }
}

/// `152.3 m`, `152 m` for a whole number.
pub fn depth_text(d: f64) -> String {
    if (d - d.round()).abs() < 0.05 {
        format!("{:.0} m", d)
    } else {
        format!("{:.1} m", d)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub x: f64,
    pub y: f64,
    pub crs: Crs,
    pub lon: f64,
    pub lat: f64,
    pub source: String,
    pub datasheet_name: String,
    pub datasheet_file: String,
    /// Drill core: the depth along the hole this specimen comes from, and the direction of the hole.
    pub depth: Option<f64>,
    pub hole: Option<Hole>,
    /// Anything that is not drill core (a block sample, the Owens method): the elevation above sea level in metres.
    pub elevation: Option<f64>,
}

impl Location {
    /// The one extra number typed next to the coordinates: the depth along the hole for drill core, else the
    /// elevation. The other one is cleared.
    pub fn set_vertical(&mut self, core: bool, v: Option<f64>) {
        self.depth = if core { v } else { None };
        self.elevation = if core { None } else { v };
    }

    /// A location from coordinates in a system; `None` when they do not give a point on the earth.
    pub fn new(x: f64, y: f64, crs: Crs, source: &str) -> Option<Location> {
        // UTM has a valid range (easting 100 to 900 km, northing 0 to 10000 km): a swapped pair is caught here
        if let Crs::Utm { .. } = crs {
            if !(100_000.0..900_000.0).contains(&x) || !(0.0..=10_000_000.0).contains(&y) {
                return None;
            }
        }
        let (lon, lat) = crs.to_lonlat(x, y);
        (lon.is_finite() && lat.is_finite() && lat.abs() < gis::MAX_LAT && lon.abs() <= 180.0).then(|| Location { x, y, crs, lon, lat, source: source.into(), datasheet_name: String::new(), datasheet_file: String::new(), depth: None, hole: None, elevation: None })
    }

    pub fn merc(&self) -> (f64, f64) {
        gis::merc_from_lonlat(self.lon, self.lat)
    }

    /// `UTM 33N  412345.7, 4851234.6  (43.80898 N, 13.91019 E)`.
    pub fn summary(&self) -> String {
        let (ns, ew) = (if self.lat >= 0.0 { "N" } else { "S" }, if self.lon >= 0.0 { "E" } else { "W" });
        let at = format!("{:.5} {ns}, {:.5} {ew}", self.lat.abs(), self.lon.abs());
        let at = match (self.depth, self.elevation) {
            (Some(d), _) => format!("{at}, {}", depth_text(d)),
            (None, Some(e)) => format!("{at}, {}", elevation_text(e)),
            _ => at,
        };
        match self.crs {
            Crs::LonLat => at,
            c => format!("{}  {:.1}, {:.1}  ({at})", c.short_label(), self.x, self.y),
        }
    }
}

/// The location stored in `json_path`, if any.
pub fn read(json_path: &Path) -> Option<Location> {
    let v = depjson::read_section(json_path, "location")?;
    let f = |k: &str| v.get(k).and_then(|x| x.as_f64());
    let crs = v.get("crs").and_then(crs_from_json)?;
    let (x, y) = (f("x")?, f("y")?);
    // the longitude / latitude are recomputed: they are stored for people reading the file
    let mut l = Location::new(x, y, crs, v.get("source").and_then(|s| s.as_str()).unwrap_or("typed"))?;
    l.datasheet_name = v.get("datasheet_name").and_then(|s| s.as_str()).unwrap_or("").to_string();
    l.datasheet_file = v.get("datasheet_file").and_then(|s| s.as_str()).unwrap_or("").to_string();
    l.depth = f("depth_m").filter(|d| valid_depth(*d));
    l.elevation = f("elevation_m").filter(|e| valid_elevation(*e));
    l.hole = v.get("hole").and_then(|h| Hole::new(h.get("dip_direction")?.as_f64()?, h.get("plunge")?.as_f64()?));
    Some(l)
}

/// Stores (or, with `None`, removes) the location, making the `_geo` folder and the file when needed.
pub fn write(json_path: &Path, loc: Option<&Location>) -> std::io::Result<()> {
    let v: Option<Value> = loc.map(|l| {
        let mut o = json!({ "x": l.x, "y": l.y, "crs": crs_json(&l.crs), "lon": l.lon, "lat": l.lat, "source": l.source });
        if let Some(d) = l.depth {
            o["depth_m"] = json!(d);
        }
        if let Some(e) = l.elevation {
            o["elevation_m"] = json!(e);
        }
        if let Some(h) = l.hole {
            o["hole"] = json!({ "dip_direction": h.dip_dir, "plunge": h.plunge });
        }
        if !l.datasheet_name.is_empty() {
            o["datasheet_name"] = json!(l.datasheet_name);
            o["datasheet_file"] = json!(l.datasheet_file);
        }
        o
    });
    depjson::write_section(json_path, "location", v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_is_stored_in_the_dependency_file_next_to_other_sections() {
        let base = std::env::temp_dir().join(format!("gis-loc-test-{}", std::process::id()));
        let src = base.join("ZK105_AMS").join("ZK105_use.ams");
        let json = depjson::for_source(&src);
        assert!(read(&json).is_none());
        let mut l = Location::new(412345.678, 4851234.567, Crs::Utm { zone: 33, north: true }, "datasheet").unwrap();
        l.datasheet_name = "K105".into();
        l.datasheet_file = "sheet.xlsx".into();
        assert!((l.lon - 13.9102).abs() < 1e-3 && (l.lat - 43.8090).abs() < 1e-3);
        write(&json, Some(&l)).unwrap();
        // another section in the same file stays when the location is removed
        depjson::write_section(&json, "core_viewer", Some(serde_json::json!({"photo_folder": "/x"}))).unwrap();
        let back = read(&json).unwrap();
        assert_eq!((back.x, back.y, back.crs, back.source.as_str(), back.datasheet_name.as_str()), (412345.678, 4851234.567, Crs::Utm { zone: 33, north: true }, "datasheet", "K105"));
        write(&json, None).unwrap();
        assert!(read(&json).is_none());
        assert!(depjson::read_section(&json, "core_viewer").is_some());
        // an EPSG system and a point that is not on the earth
        let e = Location::new(14412345.0, 5300123.0, Crs::Epsg(28414), "typed").unwrap();
        write(&json, Some(&e)).unwrap();
        assert_eq!(read(&json).unwrap().crs, Crs::Epsg(28414));
        assert!(Location::new(1e12, 1e12, Crs::Utm { zone: 44, north: true }, "typed").is_none());
        // northing and easting the wrong way round
        assert!(Location::new(4851234.0, 412345.0, Crs::Utm { zone: 33, north: true }, "typed").is_none());
        assert!(e.summary().contains("EPSG:28414") && l.summary().contains("UTM 33N"));
        // drill core: the depth and the direction of the hole are kept and read back; bad values are dropped on reading
        let mut c = Location::new(412345.678, 4851234.567, Crs::Utm { zone: 33, north: true }, "typed").unwrap();
        c.depth = Some(152.3);
        c.hole = Hole::new(316.1, 67.0);
        write(&json, Some(&c)).unwrap();
        let back = read(&json).unwrap();
        assert_eq!((back.depth, back.hole), (Some(152.3), Some(Hole { dip_dir: 316.1, plunge: 67.0 })));
        assert!(back.summary().contains("152.3 m"));
        assert_eq!(read(&json).unwrap(), c);
        // a block sample: an elevation instead of a depth (negative is below sea level); out of range is dropped
        let mut b = Location::new(412345.678, 4851234.567, Crs::Utm { zone: 33, north: true }, "typed").unwrap();
        b.set_vertical(false, Some(-12.5));
        assert_eq!((b.depth, b.elevation), (None, Some(-12.5)));
        write(&json, Some(&b)).unwrap();
        assert_eq!(read(&json).unwrap(), b);
        assert!(b.summary().contains("-12.5 m asl"));
        b.set_vertical(true, Some(3.0));
        assert_eq!((b.depth, b.elevation), (Some(3.0), None));
        assert!(valid_elevation(-12.5) && valid_elevation(2400.0) && !valid_elevation(12_000.0) && !valid_elevation(f64::NAN));
        assert_eq!((elevation_text(152.0), elevation_text(-12.54)), ("152 m asl".to_string(), "-12.5 m asl".to_string()));
        depjson::write_section(&json, "location", Some(serde_json::json!({"x": 412345.678, "y": 4851234.567, "crs": crs_json(&Crs::Utm { zone: 33, north: true }), "depth_m": -4.0, "hole": {"dip_direction": 400.0, "plunge": 67.0}}))).unwrap();
        let bad = read(&json).unwrap();
        assert_eq!((bad.depth, bad.hole), (None, None));
        assert!(Hole::new(0.0, 90.0).is_some() && Hole::new(360.0, 0.0).is_some() && Hole::new(10.0, 91.0).is_none() && Hole::new(-1.0, 10.0).is_none());
        assert_eq!((depth_text(152.0), depth_text(152.34), depth_text(0.0)), ("152 m".to_string(), "152.3 m".to_string(), "0 m".to_string()));
        assert!(valid_depth(0.0) && valid_depth(1234.5) && !valid_depth(-0.1) && !valid_depth(f64::NAN));
        let _ = std::fs::remove_dir_all(&base);
    }
}
