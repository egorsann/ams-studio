//! Where a specimen was taken: the `location` section of the specimen's dependency file
//! (`<specimen folder>_geo/<specimen folder>_geo_dependancy.json`, see `ams_core::depjson`), kept with the data like
//! the core viewer link and the stereonet planes. The `.ams` record has no field for a position (its free bytes are
//! used by the Owens block, and a position belongs to the specimen, not to each record), and the source files are
//! never written.
//!
//! ```json
//! "location": { "x": 412345.678, "y": 4851234.567, "crs": { "type": "utm", "zone": 33, "north": true },
//!               "lon": 13.91019, "lat": 43.80898, "source": "typed" }
//! ```
//!
//! `source` is `typed`, or `datasheet` with the name of the row and the file it was taken from.

use super::gis_data::{crs_from_json, crs_json};
use ams_core::depjson;
use ams_core::gis::{self, Crs};
use serde_json::{json, Value};
use std::path::Path;

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
}

impl Location {
    /// A location from coordinates in a system; `None` when they do not give a point on the earth.
    pub fn new(x: f64, y: f64, crs: Crs, source: &str) -> Option<Location> {
        // UTM has a valid range (easting 100 to 900 km, northing 0 to 10000 km): a swapped pair is caught here
        if let Crs::Utm { .. } = crs {
            if !(100_000.0..900_000.0).contains(&x) || !(0.0..=10_000_000.0).contains(&y) {
                return None;
            }
        }
        let (lon, lat) = crs.to_lonlat(x, y);
        (lon.is_finite() && lat.is_finite() && lat.abs() < gis::MAX_LAT && lon.abs() <= 180.0).then(|| Location { x, y, crs, lon, lat, source: source.into(), datasheet_name: String::new(), datasheet_file: String::new() })
    }

    pub fn merc(&self) -> (f64, f64) {
        gis::merc_from_lonlat(self.lon, self.lat)
    }

    /// `UTM 33N  412345.7, 4851234.6  (43.80898 N, 13.91019 E)`.
    pub fn summary(&self) -> String {
        let (ns, ew) = (if self.lat >= 0.0 { "N" } else { "S" }, if self.lon >= 0.0 { "E" } else { "W" });
        let at = format!("{:.5} {ns}, {:.5} {ew}", self.lat.abs(), self.lon.abs());
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
    Some(l)
}

/// Stores (or, with `None`, removes) the location, making the `_geo` folder and the file when needed.
pub fn write(json_path: &Path, loc: Option<&Location>) -> std::io::Result<()> {
    let v: Option<Value> = loc.map(|l| {
        let mut o = json!({ "x": l.x, "y": l.y, "crs": crs_json(&l.crs), "lon": l.lon, "lat": l.lat, "source": l.source });
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
        let _ = std::fs::remove_dir_all(&base);
    }
}
