//! Vector data for the GIS viewer: shapefiles (`.shp`, with the `.prj` next to it), GeoPackages (`.gpkg`, every feature
//! table is a layer) and GeoJSON (`.geojson`, `.json`). Points, lines and polygons are read into plain coordinate lists
//! (every feature's attribute row is kept too, for filters and labels), the coordinate system comes from the file when it says one, and a
//! polygon is cut into triangles once so that it can be filled however concave it is or however many holes it has.
//!
//! Nothing here opens a file for writing: a GeoPackage is opened read-only.

use ams_core::gis::Crs;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One attribute value of a feature.
#[derive(Clone, Debug, PartialEq)]
pub enum AttrVal {
    Null,
    Num(f64),
    Text(String),
}

impl AttrVal {
    /// How the value is written (labels, tooltips).
    pub fn text(&self) -> String {
        match self {
            AttrVal::Null => String::new(),
            AttrVal::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            AttrVal::Num(n) => format!("{n}"),
            AttrVal::Text(t) => t.clone(),
        }
    }
}

/// Coordinates as they are in the file (in its own coordinate system), and the attributes of the features they belong
/// to. A feature can hold several geometries (a multipolygon), so `pfeat` / `lfeat` / `gfeat` say for every point, line
/// and polygon which row of `rows` it belongs to (empty when the file has no attributes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorData {
    pub points: Vec<[f64; 2]>,
    pub lines: Vec<Vec<[f64; 2]>>,
    /// A polygon is its rings: the first is the outer ring, the others are holes.
    pub polys: Vec<Vec<Vec<[f64; 2]>>>,
    /// The attribute names, and one row of values per feature.
    pub fields: Vec<String>,
    pub rows: Vec<Vec<AttrVal>>,
    pub pfeat: Vec<u32>,
    pub lfeat: Vec<u32>,
    pub gfeat: Vec<u32>,
}

impl VectorData {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.lines.is_empty() && self.polys.is_empty()
    }

    /// Everything that was added since the last call belongs to feature `feat`.
    fn fill(&mut self, feat: u32) {
        self.pfeat.resize(self.points.len(), feat);
        self.lfeat.resize(self.lines.len(), feat);
        self.gfeat.resize(self.polys.len(), feat);
    }

    /// The index of an attribute by name.
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f == name)
    }

    /// Only the geometry of the features with `keep[row]` set (the rows and their numbers stay as they are). A file
    /// without attributes, or a mask of the wrong length, keeps everything.
    pub fn filtered(&self, keep: &[bool]) -> VectorData {
        if self.rows.is_empty() || keep.len() != self.rows.len() {
            return self.clone();
        }
        let ok = |f: &u32| keep.get(*f as usize).copied().unwrap_or(true);
        let mut out = VectorData { fields: self.fields.clone(), rows: self.rows.clone(), ..Default::default() };
        for (g, f) in self.points.iter().zip(&self.pfeat) {
            if ok(f) {
                out.points.push(*g);
                out.pfeat.push(*f);
            }
        }
        for (g, f) in self.lines.iter().zip(&self.lfeat) {
            if ok(f) {
                out.lines.push(g.clone());
                out.lfeat.push(*f);
            }
        }
        for (g, f) in self.polys.iter().zip(&self.gfeat) {
            if ok(f) {
                out.polys.push(g.clone());
                out.gfeat.push(*f);
            }
        }
        out
    }
}

/// One layer read from a file.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// The file it came from.
    pub path: PathBuf,
    pub name: String,
    /// The table of a GeoPackage (empty for the other formats).
    pub table: String,
    pub data: VectorData,
    /// `None` when the file does not say (or says something that is not supported): the user is asked.
    pub crs: Option<Crs>,
    pub note: String,
}

/// File extensions that are vector data.
pub const VECTOR_EXTS: [&str; 4] = ["shp", "gpkg", "geojson", "json"];

pub fn is_vector(path: &Path) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| VECTOR_EXTS.contains(&e.as_str()))
}

/// Reads the layers of a file (a GeoPackage can hold several). `only` names the one table wanted, when there are many.
pub fn read_vector(path: &Path, only: Option<&str>) -> Result<Vec<Loaded>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Layer".into());
    let layers = match ext.as_str() {
        "shp" => read_shp(path, &stem).map(|l| vec![l]),
        "geojson" | "json" => read_geojson(path, &stem).map(|l| vec![l]),
        "gpkg" => read_gpkg(path, &stem, only),
        _ => Err(format!("{}: not a shapefile, GeoPackage or GeoJSON file", path.display())),
    };
    layers.map(|mut v| {
        for l in &mut v {
            l.path = path.to_path_buf();
        }
        v
    })
}

// ------------------------------------------------------------------------------------------------ WKB

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.p.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("the geometry is cut off")?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
}

fn u32_at(rd: &mut Rd, le: bool) -> Result<u32, String> {
    let b: [u8; 4] = rd.take(4)?.try_into().map_err(|_| "short read".to_string())?;
    Ok(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
}

fn f64_at(rd: &mut Rd, le: bool) -> Result<f64, String> {
    let b: [u8; 8] = rd.take(8)?.try_into().map_err(|_| "short read".to_string())?;
    Ok(if le { f64::from_le_bytes(b) } else { f64::from_be_bytes(b) })
}

fn coords(rd: &mut Rd, le: bool, dims: usize, n: usize) -> Result<Vec<[f64; 2]>, String> {
    // every point needs `dims` numbers of 8 bytes: a count that cannot fit is a damaged geometry
    if n.checked_mul(dims * 8).map_or(true, |need| need > rd.b.len() - rd.p) {
        return Err("the geometry is cut off".into());
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let (x, y) = (f64_at(rd, le)?, f64_at(rd, le)?);
        for _ in 2..dims {
            f64_at(rd, le)?;
        }
        v.push([x, y]);
    }
    Ok(v)
}

fn wkb_geometry(rd: &mut Rd, out: &mut VectorData, depth: u32) -> Result<(), String> {
    if depth > 8 {
        return Err("geometries nested too deeply".into());
    }
    let le = match rd.take(1)?[0] {
        0 => false,
        1 => true,
        _ => return Err("not a WKB geometry".into()),
    };
    let t = u32_at(rd, le)?;
    // EWKB flags (Z, M, SRID in the high bits) and ISO codes (1000 Z, 2000 M, 3000 ZM)
    let (ez, em, es) = (t & 0x8000_0000 != 0, t & 0x4000_0000 != 0, t & 0x2000_0000 != 0);
    let code = t & 0x0fff_ffff;
    let (base, iso) = (code % 1000, code / 1000);
    let dims = 2 + usize::from(ez || iso == 1 || iso == 3) + usize::from(em || iso == 2 || iso == 3);
    if es {
        u32_at(rd, le)?;
    }
    match base {
        1 => {
            let p = coords(rd, le, dims, 1)?[0];
            // an empty point is written as NaN
            if p[0].is_finite() && p[1].is_finite() {
                out.points.push(p);
            }
        }
        2 => {
            let n = u32_at(rd, le)? as usize;
            let l = coords(rd, le, dims, n)?;
            if l.len() >= 2 {
                out.lines.push(l);
            }
        }
        3 => {
            let nr = u32_at(rd, le)? as usize;
            let mut rings = vec![];
            for _ in 0..nr {
                let n = u32_at(rd, le)? as usize;
                let r = coords(rd, le, dims, n)?;
                if r.len() >= 3 {
                    rings.push(r);
                }
            }
            if !rings.is_empty() {
                out.polys.push(rings);
            }
        }
        4..=7 => {
            let n = u32_at(rd, le)? as usize;
            for _ in 0..n {
                wkb_geometry(rd, out, depth + 1)?;
            }
        }
        other => return Err(format!("geometry type {other} (curves and surfaces) is not supported")),
    }
    Ok(())
}

/// Reads one WKB geometry (Point, LineString, Polygon and their Multi and collection forms, 2D, Z, M and ZM).
pub fn parse_wkb(b: &[u8], out: &mut VectorData) -> Result<(), String> {
    wkb_geometry(&mut Rd { b, p: 0 }, out, 0)
}

/// The geometry of a GeoPackage blob: the standard header (magic `GP`, flags, SRS id, an envelope) and then WKB.
/// Returns the SRS id; an empty geometry gives nothing.
pub fn parse_gpkg_blob(b: &[u8], out: &mut VectorData) -> Result<i32, String> {
    if b.len() < 8 || &b[..2] != b"GP" {
        return Err("not a GeoPackage geometry".into());
    }
    let flags = b[3];
    let le = flags & 1 == 1;
    let srs = i32::from_le_bytes(b[4..8].try_into().unwrap_or([0; 4]));
    let srs = if le { srs } else { i32::from_be_bytes(b[4..8].try_into().unwrap_or([0; 4])) };
    let env = match (flags >> 1) & 7 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return Err("unknown envelope in a GeoPackage geometry".into()),
    };
    if flags & 0x10 != 0 || b.len() <= 8 + env {
        return Ok(srs);
    }
    parse_wkb(&b[8 + env..], out)?;
    Ok(srs)
}

// ------------------------------------------------------------------------------------------------ shapefile

fn signed_area(r: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..r.len() {
        let (p, q) = (r[i], r[(i + 1) % r.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// Reads the geometry of a `.shp` (points, multipoints, polylines and polygons, also the Z and M kinds). The
/// coordinate system comes from the `.prj` next to it.
fn read_shp(path: &Path, name: &str) -> Result<Loaded, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() < 100 || b[..4] != [0, 0, 0x27, 0x0a] {
        return Err(format!("{}: not a shapefile", path.display()));
    }
    let mut data = VectorData::default();
    let (mut skipped, mut p, mut recno) = (0usize, 100usize, 0u32);
    let le_i32 = |s: &[u8]| i32::from_le_bytes(s.try_into().unwrap_or([0; 4]));
    let le_f64 = |s: &[u8]| f64::from_le_bytes(s.try_into().unwrap_or([0; 8]));
    while p + 12 <= b.len() {
        // record header: number and content length in 16 bit words, both big endian
        let words = u32::from_be_bytes(b[p + 4..p + 8].try_into().unwrap_or([0; 4])) as usize;
        let (start, end) = (p + 8, (p + 8).saturating_add(words * 2));
        if end > b.len() || words == 0 {
            break;
        }
        let rec = &b[start..end];
        p = end;
        // what the previous record added belongs to it (records are numbered in file order, as the .dbf rows are)
        data.fill(recno);
        recno += 1;
        let kind = le_i32(&rec[..4]);
        let pt = |o: usize| -> Option<[f64; 2]> { (rec.len() >= o + 16).then(|| [le_f64(&rec[o..o + 8]), le_f64(&rec[o + 8..o + 16])]) };
        match kind {
            0 => {}
            1 | 11 | 21 => {
                if let Some(q) = pt(4) {
                    data.points.push(q);
                }
            }
            8 | 18 | 28 => {
                let n = rec.get(36..40).map_or(0, |s| le_i32(s).max(0) as usize);
                for i in 0..n {
                    if let Some(q) = pt(40 + i * 16) {
                        data.points.push(q);
                    }
                }
            }
            3 | 13 | 23 | 5 | 15 | 25 => {
                if rec.len() < 44 {
                    skipped += 1;
                    continue;
                }
                let (nparts, npts) = (le_i32(&rec[36..40]).max(0) as usize, le_i32(&rec[40..44]).max(0) as usize);
                let parts_at = 44;
                let pts_at = parts_at + nparts * 4;
                if pts_at + npts * 16 > rec.len() || nparts == 0 {
                    skipped += 1;
                    continue;
                }
                let starts: Vec<usize> = (0..nparts).map(|i| le_i32(&rec[parts_at + i * 4..parts_at + i * 4 + 4]).max(0) as usize).collect();
                let part = |i: usize| -> Vec<[f64; 2]> {
                    let (a, z) = (starts[i].min(npts), if i + 1 < nparts { starts[i + 1].min(npts) } else { npts });
                    (a..z).filter_map(|k| pt(pts_at + k * 16)).collect()
                };
                if matches!(kind, 3 | 13 | 23) {
                    for i in 0..nparts {
                        let l = part(i);
                        if l.len() >= 2 {
                            data.lines.push(l);
                        }
                    }
                } else {
                    // outer rings run clockwise, holes counter-clockwise: a hole belongs to the outer ring before it
                    let mut cur: Vec<Vec<[f64; 2]>> = vec![];
                    for i in 0..nparts {
                        let r = part(i);
                        if r.len() < 3 {
                            continue;
                        }
                        let outer = signed_area(&r) <= 0.0;
                        if outer && !cur.is_empty() {
                            data.polys.push(std::mem::take(&mut cur));
                        }
                        cur.push(r);
                    }
                    if !cur.is_empty() {
                        data.polys.push(cur);
                    }
                }
            }
            _ => skipped += 1,
        }
    }
    data.fill(recno.saturating_sub(1));
    if let Some((fields, rows)) = read_dbf(&path.with_extension("dbf")).or_else(|| read_dbf(&path.with_extension("DBF"))) {
        data.fields = fields;
        data.rows = rows;
    }
    let (crs, mut note) = match std::fs::read_to_string(path.with_extension("prj")) {
        Ok(t) => match Crs::from_prj(&t) {
            Some(c) => (Some(c), format!("Coordinate system from the .prj: {}.", c.label())),
            None => (None, "The .prj names a coordinate system that is not known here.".to_string()),
        },
        Err(_) => (None, "There is no .prj next to the file.".to_string()),
    };
    if skipped > 0 {
        note += &format!(" (n={skipped}) shapes of a kind that is not drawn were skipped.");
    }
    if data.is_empty() {
        return Err(format!("{}: no points, lines or polygons in the file", path.display()));
    }
    Ok(Loaded { path: PathBuf::new(), name: name.to_string(), table: String::new(), data, crs, note })
}

/// The attribute table of a shapefile (`.dbf`): the field names and a row per record, in record order (a record that is
/// marked deleted is a row of nulls so the numbers still line up). Text is UTF-8, or Latin-1 when it is not UTF-8.
fn read_dbf(path: &Path) -> Option<(Vec<String>, Vec<Vec<AttrVal>>)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 33 {
        return None;
    }
    let n = u32::from_le_bytes(b[4..8].try_into().ok()?) as usize;
    let (head, rec_len) = (u16::from_le_bytes(b[8..10].try_into().ok()?) as usize, u16::from_le_bytes(b[10..12].try_into().ok()?) as usize);
    let text = |s: &[u8]| -> String {
        let s = match s.iter().position(|c| *c == 0) {
            Some(z) => &s[..z],
            None => s,
        };
        match std::str::from_utf8(s) {
            Ok(t) => t.trim().to_string(),
            Err(_) => s.iter().map(|&c| c as char).collect::<String>().trim().to_string(),
        }
    };
    // field descriptors: 32 bytes each, from byte 32 to the 0x0D terminator
    let mut fields: Vec<(String, u8, usize)> = vec![];
    let mut off = 1usize;
    let mut p = 32;
    while p + 32 <= head.min(b.len()) && b[p] != 0x0d {
        let len = b[p + 16] as usize;
        fields.push((text(&b[p..p + 11]), b[p + 11], len));
        p += 32;
    }
    if fields.is_empty() || rec_len == 0 {
        return None;
    }
    let mut rows = Vec::with_capacity(n.min(1_000_000));
    for i in 0..n {
        let start = head + i * rec_len;
        let Some(rec) = b.get(start..start + rec_len) else { break };
        if rec[0] == b'*' {
            rows.push(vec![AttrVal::Null; fields.len()]);
            continue;
        }
        off = 1;
        let mut row = Vec::with_capacity(fields.len());
        for (_, ty, len) in &fields {
            let cell = rec.get(off..off + len).unwrap_or(&[]);
            off += len;
            let t = text(cell);
            row.push(match ty {
                b'N' | b'F' | b'O' | b'I' | b'B' => t.replace(',', ".").parse::<f64>().map_or(AttrVal::Null, AttrVal::Num),
                b'L' => match t.as_str() {
                    "Y" | "y" | "T" | "t" => AttrVal::Num(1.0),
                    "N" | "n" | "F" | "f" => AttrVal::Num(0.0),
                    _ => AttrVal::Null,
                },
                _ => {
                    if t.is_empty() {
                        AttrVal::Null
                    } else {
                        AttrVal::Text(t)
                    }
                }
            });
        }
        rows.push(row);
    }
    let _ = off;
    Some((fields.into_iter().map(|f| f.0).collect(), rows))
}

// ------------------------------------------------------------------------------------------------ GeoJSON

fn json_pt(v: &serde_json::Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

fn json_line(v: &serde_json::Value) -> Vec<[f64; 2]> {
    v.as_array().map(|a| a.iter().filter_map(json_pt).collect()).unwrap_or_default()
}

fn json_geometry(g: &serde_json::Value, out: &mut VectorData, depth: u32) {
    if depth > 8 {
        return;
    }
    let c = &g["coordinates"];
    let polygon = |v: &serde_json::Value| -> Vec<Vec<[f64; 2]>> { v.as_array().map(|r| r.iter().map(json_line).filter(|r| r.len() >= 3).collect()).unwrap_or_default() };
    match g["type"].as_str() {
        Some("Point") => out.points.extend(json_pt(c)),
        Some("MultiPoint") => out.points.extend(c.as_array().into_iter().flatten().filter_map(json_pt)),
        Some("LineString") => {
            let l = json_line(c);
            if l.len() >= 2 {
                out.lines.push(l);
            }
        }
        Some("MultiLineString") => out.lines.extend(c.as_array().into_iter().flatten().map(json_line).filter(|l| l.len() >= 2)),
        Some("Polygon") => {
            let p = polygon(c);
            if !p.is_empty() {
                out.polys.push(p);
            }
        }
        Some("MultiPolygon") => out.polys.extend(c.as_array().into_iter().flatten().map(polygon).filter(|p| !p.is_empty())),
        Some("GeometryCollection") => {
            for x in g["geometries"].as_array().into_iter().flatten() {
                json_geometry(x, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn read_geojson(path: &Path, name: &str) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: not valid JSON ({e})", path.display()))?;
    let mut data = VectorData::default();
    let mut props: Vec<serde_json::Value> = vec![];
    match v["type"].as_str() {
        Some("FeatureCollection") => {
            for (i, f) in v["features"].as_array().into_iter().flatten().enumerate() {
                json_geometry(&f["geometry"], &mut data, 0);
                data.fill(i as u32);
                props.push(f["properties"].clone());
            }
        }
        Some("Feature") => {
            json_geometry(&v["geometry"], &mut data, 0);
            data.fill(0);
            props.push(v["properties"].clone());
        }
        Some(_) => json_geometry(&v, &mut data, 0),
        None => return Err(format!("{}: not GeoJSON", path.display())),
    }
    // the attributes: every property name that occurs, in the order of first appearance
    for p in &props {
        for k in p.as_object().into_iter().flat_map(|o| o.keys()) {
            if !data.fields.contains(k) {
                data.fields.push(k.clone());
            }
        }
    }
    if !data.fields.is_empty() {
        let fields = data.fields.clone();
        data.rows = props
            .iter()
            .map(|p| {
                fields
                    .iter()
                    .map(|k| match &p[k.as_str()] {
                        serde_json::Value::Null => AttrVal::Null,
                        serde_json::Value::Number(n) => n.as_f64().map_or(AttrVal::Null, AttrVal::Num),
                        serde_json::Value::Bool(b) => AttrVal::Num(f64::from(u8::from(*b))),
                        serde_json::Value::String(t) => AttrVal::Text(t.clone()),
                        other => AttrVal::Text(other.to_string()),
                    })
                    .collect()
            })
            .collect();
    }
    if data.is_empty() {
        return Err(format!("{}: no points, lines or polygons in the file", path.display()));
    }
    // GeoJSON is longitude / latitude on WGS 84; the old "crs" member may name another system
    let named = v["crs"]["properties"]["name"].as_str();
    let (crs, note) = match named {
        None => (Some(Crs::LonLat), "GeoJSON: longitude / latitude (WGS 84).".to_string()),
        Some(n) => {
            let code: Option<u32> = n.rsplit(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty()).and_then(|s| s.parse().ok());
            match code.and_then(Crs::from_epsg) {
                Some(c) => (Some(c), format!("Coordinate system named in the file: {}.", c.label())),
                None => (None, format!("The file names the coordinate system {n}, which is not known here.")),
            }
        }
    };
    Ok(Loaded { path: PathBuf::new(), name: name.to_string(), table: String::new(), data, crs, note })
}

// ------------------------------------------------------------------------------------------------ GeoPackage

fn quote(id: &str) -> String {
    format!("\"{}\"", id.replace('"', "\"\""))
}

fn read_gpkg(path: &Path, stem: &str, only: Option<&str>) -> Result<Vec<Loaded>, String> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut st = db.prepare("SELECT table_name FROM gpkg_contents WHERE data_type = 'features'").map_err(|_| format!("{}: not a GeoPackage (no feature tables)", path.display()))?;
    let tables: Vec<String> = st.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.filter_map(|t| t.ok()).collect();
    drop(st);
    let mut out = vec![];
    for table in tables.iter().filter(|t| only.map_or(true, |o| o == t.as_str())) {
        let gc: Option<(String, i32)> = db.query_row("SELECT column_name, srs_id FROM gpkg_geometry_columns WHERE table_name = ?1", [table], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        let Some((col, srs_id)) = gc else { continue };
        let mut data = VectorData::default();
        // the other columns are the attributes
        let cols: Vec<String> = db
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(1)).map(|it| it.filter_map(|c| c.ok()).collect()))
            .unwrap_or_default();
        data.fields = cols.into_iter().filter(|c| *c != col).collect();
        let list: String = std::iter::once(quote(&col)).chain(data.fields.iter().map(|c| quote(c))).collect::<Vec<_>>().join(", ");
        let sql = format!("SELECT {list} FROM {}", quote(table));
        let mut q = db.prepare(&sql).map_err(|e| format!("{table}: {e}"))?;
        let mut rows = q.query([]).map_err(|e| format!("{table}: {e}"))?;
        let mut bad = 0usize;
        let mut feat = 0u32;
        while let Some(r) = rows.next().map_err(|e| format!("{table}: {e}"))? {
            if let Ok(Some(blob)) = r.get::<_, Option<Vec<u8>>>(0) {
                if parse_gpkg_blob(&blob, &mut data).is_err() {
                    bad += 1;
                }
            }
            data.fill(feat);
            feat += 1;
            let row: Vec<AttrVal> = (1..=data.fields.len())
                .map(|i| match r.get_ref(i) {
                    Ok(rusqlite::types::ValueRef::Integer(n)) => AttrVal::Num(n as f64),
                    Ok(rusqlite::types::ValueRef::Real(x)) => AttrVal::Num(x),
                    Ok(rusqlite::types::ValueRef::Text(t)) => AttrVal::Text(String::from_utf8_lossy(t).to_string()),
                    Ok(rusqlite::types::ValueRef::Blob(_)) => AttrVal::Text("(blob)".into()),
                    _ => AttrVal::Null,
                })
                .collect();
            data.rows.push(row);
        }
        if data.is_empty() {
            continue;
        }
        // the coordinate system from the table of reference systems: an EPSG code, or a definition that is read like a .prj
        let srs: Option<(String, i64, String)> = db.query_row("SELECT organization, organization_coordsys_id, definition FROM gpkg_spatial_ref_sys WHERE srs_id = ?1", [srs_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
        let crs = srs.and_then(|(org, code, def)| {
            let by_code = org.eq_ignore_ascii_case("EPSG").then(|| u32::try_from(code).ok().and_then(Crs::from_epsg)).flatten();
            by_code.or_else(|| Crs::from_prj(&def))
        });
        let mut note = match crs {
            Some(c) => format!("Coordinate system of the table: {}.", c.label()),
            None => "The table's coordinate system is not known here.".to_string(),
        };
        if bad > 0 {
            note += &format!(" (n={bad}) geometries could not be read.");
        }
        let name = if tables.len() > 1 { format!("{stem} - {table}") } else { stem.to_string() };
        out.push(Loaded { path: PathBuf::new(), name, table: table.clone(), data, crs, note });
    }
    if out.is_empty() {
        return Err(format!("{}: no feature table with points, lines or polygons", path.display()));
    }
    Ok(out)
}

/// A coordinate system as a short word for the preferences file (`None` for a user-typed PROJ string, which only lives
/// for the session).
pub fn crs_token(c: Crs) -> Option<String> {
    Some(match c {
        Crs::LonLat => "lonlat".to_string(),
        Crs::WebMerc => "merc".to_string(),
        Crs::Utm { zone, north } => format!("utm:{zone}{}", if north { 'n' } else { 's' }),
        Crs::Epsg(code) => format!("epsg:{code}"),
        Crs::Custom(_) => return None,
    })
}

pub fn crs_from_token(t: &str) -> Option<Crs> {
    match t {
        "lonlat" => Some(Crs::LonLat),
        "merc" => Some(Crs::WebMerc),
        _ => {
            if let Some(u) = t.strip_prefix("utm:") {
                let north = match u.chars().last()? {
                    'n' => true,
                    's' => false,
                    _ => return None,
                };
                let zone: u8 = u[..u.len() - 1].parse().ok()?;
                return (1..=60).contains(&zone).then_some(Crs::Utm { zone, north });
            }
            t.strip_prefix("epsg:").and_then(|c| c.parse::<u32>().ok()).and_then(Crs::from_epsg)
        }
    }
}

// ------------------------------------------------------------------------------------------------ in Web Mercator

/// A polygon in Web Mercator metres, cut into triangles.
#[derive(Clone, Debug)]
pub struct PolyM {
    /// All vertices of all rings (the closing vertex of a ring is dropped).
    pub verts: Vec<[f64; 2]>,
    /// Where each ring is in `verts`.
    pub rings: Vec<Range<usize>>,
    /// Triangles as indices into `verts`, three each.
    pub tris: Vec<u32>,
    pub bbox: [f64; 4],
}

#[derive(Clone, Debug)]
pub struct LineM {
    pub pts: Vec<[f64; 2]>,
    pub bbox: [f64; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Projected {
    pub points: Vec<[f64; 2]>,
    pub lines: Vec<LineM>,
    pub polys: Vec<PolyM>,
    /// The feature (row of the attribute table) of every point, line and polygon above; empty without attributes.
    pub pfeat: Vec<u32>,
    pub lfeat: Vec<u32>,
    pub gfeat: Vec<u32>,
    /// (min, max) of everything.
    pub bbox: Option<([f64; 2], [f64; 2])>,
}

fn bbox_of<'a>(pts: impl Iterator<Item = &'a [f64; 2]>) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in pts {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    b
}

/// Moves everything into Web Mercator metres (what the map is drawn in); points that cannot be converted are dropped.
pub fn project(data: &VectorData, crs: Crs) -> Projected {
    let conv = |p: &[f64; 2]| -> Option<[f64; 2]> {
        let (x, y) = crs.to_merc(p[0], p[1]);
        (x.is_finite() && y.is_finite()).then_some([x, y])
    };
    let has = !data.rows.is_empty();
    let mut out = Projected::default();
    for (i, p) in data.points.iter().enumerate() {
        if let Some(q) = conv(p) {
            out.points.push(q);
            if has {
                out.pfeat.push(data.pfeat.get(i).copied().unwrap_or(0));
            }
        }
    }
    for (i, l) in data.lines.iter().enumerate() {
        let pts: Vec<[f64; 2]> = l.iter().filter_map(conv).collect();
        if pts.len() >= 2 {
            out.lines.push(LineM { bbox: bbox_of(pts.iter()), pts });
            if has {
                out.lfeat.push(data.lfeat.get(i).copied().unwrap_or(0));
            }
        }
    }
    for (pi, poly) in data.polys.iter().enumerate() {
        let (mut verts, mut rings, mut holes) = (vec![], vec![], vec![]);
        for (k, ring) in poly.iter().enumerate() {
            let mut r: Vec<[f64; 2]> = ring.iter().filter_map(conv).collect();
            if r.len() > 1 && r.first() == r.last() {
                r.pop();
            }
            if r.len() < 3 {
                if k == 0 {
                    break;
                }
                continue;
            }
            if k > 0 {
                holes.push(verts.len());
            }
            rings.push(verts.len()..verts.len() + r.len());
            verts.extend(r);
        }
        if rings.is_empty() {
            continue;
        }
        let flat: Vec<f64> = verts.iter().flat_map(|v| [v[0], v[1]]).collect();
        let tris = earcutr::earcut(&flat, &holes, 2).map(|t| t.into_iter().map(|i| i as u32).collect()).unwrap_or_default();
        out.polys.push(PolyM { bbox: bbox_of(verts.iter()), verts, rings, tris });
        if has {
            out.gfeat.push(data.gfeat.get(pi).copied().unwrap_or(0));
        }
    }
    let mut all = out.points.iter().map(|p| [p[0], p[1], p[0], p[1]]).chain(out.lines.iter().map(|l| l.bbox)).chain(out.polys.iter().map(|p| p.bbox));
    if let Some(first) = all.next() {
        let b = all.fold(first, |a, x| [a[0].min(x[0]), a[1].min(x[1]), a[2].max(x[2]), a[3].max(x[3])]);
        out.bbox = Some(([b[0], b[1]], [b[2], b[3]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wkb_point_le(x: f64, y: f64) -> Vec<u8> {
        let mut v = vec![1u8, 1, 0, 0, 0];
        v.extend(x.to_le_bytes());
        v.extend(y.to_le_bytes());
        v
    }

    fn wkb_polygon_le(rings: &[&[[f64; 2]]], type_code: u32) -> Vec<u8> {
        let mut v = vec![1u8];
        v.extend(type_code.to_le_bytes());
        v.extend((rings.len() as u32).to_le_bytes());
        for r in rings {
            v.extend((r.len() as u32).to_le_bytes());
            for p in *r {
                v.extend(p[0].to_le_bytes());
                v.extend(p[1].to_le_bytes());
                if type_code == 1003 {
                    v.extend(7.0f64.to_le_bytes());
                }
            }
        }
        v
    }

    const SQUARE: [[f64; 2]; 5] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]];
    const HOLE: [[f64; 2]; 5] = [[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0], [3.0, 3.0]];

    #[test]
    fn wkb_points_lines_polygons_and_their_variants() {
        let mut d = VectorData::default();
        parse_wkb(&wkb_point_le(1.5, 2.5), &mut d).unwrap();
        assert_eq!(d.points, vec![[1.5, 2.5]]);
        // big endian
        let mut be = vec![0u8, 0, 0, 0, 1];
        be.extend(3.0f64.to_be_bytes());
        be.extend(4.0f64.to_be_bytes());
        parse_wkb(&be, &mut d).unwrap();
        assert_eq!(d.points[1], [3.0, 4.0]);
        // a polygon with a hole, 2D and ISO Z (the third number of every vertex is skipped)
        let mut p = VectorData::default();
        parse_wkb(&wkb_polygon_le(&[&SQUARE, &HOLE], 3), &mut p).unwrap();
        parse_wkb(&wkb_polygon_le(&[&SQUARE], 1003), &mut p).unwrap();
        assert_eq!((p.polys.len(), p.polys[0].len(), p.polys[1].len(), p.polys[1][0][1]), (2, 2, 1, [10.0, 0.0]));
        // EWKB with an SRID, and a MultiPoint holding two points
        let mut e = vec![1u8];
        e.extend((1u32 | 0x2000_0000).to_le_bytes());
        e.extend(4326u32.to_le_bytes());
        e.extend(5.0f64.to_le_bytes());
        e.extend(6.0f64.to_le_bytes());
        let mut m = VectorData::default();
        parse_wkb(&e, &mut m).unwrap();
        assert_eq!(m.points, vec![[5.0, 6.0]]);
        let mut multi = vec![1u8, 4, 0, 0, 0, 2, 0, 0, 0];
        multi.extend(wkb_point_le(1.0, 1.0));
        multi.extend(wkb_point_le(2.0, 2.0));
        let mut mm = VectorData::default();
        parse_wkb(&multi, &mut mm).unwrap();
        assert_eq!(mm.points.len(), 2);
        // a curve is refused with a message, a cut-off geometry is an error, an empty point (NaN) is nothing
        assert!(parse_wkb(&[1, 8, 0, 0, 0, 0, 0, 0, 0], &mut VectorData::default()).unwrap_err().contains("not supported"));
        assert!(parse_wkb(&wkb_point_le(1.0, 2.0)[..10], &mut VectorData::default()).is_err());
        let mut n = VectorData::default();
        parse_wkb(&wkb_point_le(f64::NAN, f64::NAN), &mut n).unwrap();
        assert!(n.is_empty());
        // a claimed point count that cannot be there does not allocate or panic
        let mut huge = vec![1u8, 2, 0, 0, 0];
        huge.extend(u32::MAX.to_le_bytes());
        assert!(parse_wkb(&huge, &mut VectorData::default()).is_err());
    }

    #[test]
    fn a_geopackage_blob_has_a_header_before_the_wkb() {
        let wkb = wkb_point_le(8.0, 9.0);
        // flags: little endian, no envelope
        let mut b = vec![b'G', b'P', 0, 1];
        b.extend(32633i32.to_le_bytes());
        b.extend(&wkb);
        let mut d = VectorData::default();
        assert_eq!(parse_gpkg_blob(&b, &mut d).unwrap(), 32633);
        assert_eq!(d.points, vec![[8.0, 9.0]]);
        // with an XY envelope (32 bytes) in between
        let mut b = vec![b'G', b'P', 0, 1 | (1 << 1)];
        b.extend(4326i32.to_le_bytes());
        b.extend([0u8; 32]);
        b.extend(&wkb);
        let mut d = VectorData::default();
        assert_eq!(parse_gpkg_blob(&b, &mut d).unwrap(), 4326);
        assert_eq!(d.points.len(), 1);
        // an empty geometry (flag bit 4) is nothing, a blob without the magic is refused
        let mut e = vec![b'G', b'P', 0, 1 | 0x10];
        e.extend(0i32.to_le_bytes());
        assert!(parse_gpkg_blob(&e, &mut VectorData::default()).is_ok());
        assert!(parse_gpkg_blob(&[1, 2, 3], &mut VectorData::default()).is_err());
    }

    fn shp_record(kind: i32, body: &[u8], n: u32) -> Vec<u8> {
        let mut content = kind.to_le_bytes().to_vec();
        content.extend(body);
        let mut r = n.to_be_bytes().to_vec();
        r.extend(((content.len() / 2) as u32).to_be_bytes());
        r.extend(content);
        r
    }

    fn shp_file(kind: i32, records: Vec<Vec<u8>>) -> Vec<u8> {
        let mut f = vec![0u8, 0, 0x27, 0x0a];
        f.extend([0u8; 20]);
        f.extend([0u8; 4]); // length (not read)
        f.extend(1000i32.to_le_bytes());
        f.extend(kind.to_le_bytes());
        f.extend([0u8; 64]);
        for r in records {
            f.extend(r);
        }
        f
    }

    fn poly_body(parts: &[&[[f64; 2]]]) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b.extend((parts.len() as i32).to_le_bytes());
        b.extend((parts.iter().map(|p| p.len()).sum::<usize>() as i32).to_le_bytes());
        let mut at = 0i32;
        for p in parts {
            b.extend(at.to_le_bytes());
            at += p.len() as i32;
        }
        for p in parts {
            for q in *p {
                b.extend(q[0].to_le_bytes());
                b.extend(q[1].to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_shapefile_with_a_polygon_a_hole_a_line_and_points() {
        let dir = std::env::temp_dir().join(format!("gis-shp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // outer ring clockwise, hole counter-clockwise, as shapefiles are written
        let mut cw = SQUARE.to_vec();
        cw.reverse();
        let mut ccw = HOLE.to_vec();
        ccw.reverse();
        assert!(signed_area(&ccw) > 0.0 && signed_area(&cw) < 0.0);
        let poly = shp_file(5, vec![shp_record(5, &poly_body(&[&cw, &ccw]), 1), shp_record(5, &poly_body(&[&cw]), 2)]);
        std::fs::write(dir.join("units.shp"), &poly).unwrap();
        std::fs::write(dir.join("units.prj"), "PROJCS[\"WGS 84 / UTM zone 33N\",AUTHORITY[\"EPSG\",\"32633\"]]").unwrap();
        let l = read_vector(&dir.join("units.shp"), None).unwrap().remove(0);
        assert_eq!((l.data.polys.len(), l.data.polys[0].len(), l.data.polys[1].len()), (2, 2, 1));
        assert_eq!(l.crs, Some(Crs::Utm { zone: 33, north: true }));
        assert_eq!(l.name, "units");
        // a polyline file without a .prj: the coordinate system is not known
        let line = shp_file(3, vec![shp_record(3, &poly_body(&[&[[0.0, 0.0], [5.0, 5.0], [9.0, 1.0]]]), 1)]);
        std::fs::write(dir.join("faults.shp"), &line).unwrap();
        let l = read_vector(&dir.join("faults.shp"), None).unwrap().remove(0);
        assert_eq!((l.data.lines.len(), l.data.lines[0].len(), l.crs), (1, 3, None));
        assert!(l.note.contains("no .prj"));
        // points and a PointZ
        let mut pt = 1.0f64.to_le_bytes().to_vec();
        pt.extend(2.0f64.to_le_bytes());
        let mut ptz = pt.clone();
        ptz.extend(0.0f64.to_le_bytes());
        ptz.extend(0.0f64.to_le_bytes());
        std::fs::write(dir.join("pts.shp"), shp_file(1, vec![shp_record(1, &pt, 1), shp_record(1, &pt, 2)])).unwrap();
        assert_eq!(read_vector(&dir.join("pts.shp"), None).unwrap()[0].data.points.len(), 2);
        std::fs::write(dir.join("ptz.shp"), shp_file(11, vec![shp_record(11, &ptz, 1)])).unwrap();
        assert_eq!(read_vector(&dir.join("ptz.shp"), None).unwrap()[0].data.points, vec![[1.0, 2.0]]);
        // not a shapefile, and a truncated record does not panic
        std::fs::write(dir.join("junk.shp"), b"hello").unwrap();
        assert!(read_vector(&dir.join("junk.shp"), None).is_err());
        let mut cut = poly.clone();
        cut.truncate(cut.len() - 30);
        std::fs::write(dir.join("cut.shp"), cut).unwrap();
        let _ = read_vector(&dir.join("cut.shp"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geojson_features_geometries_and_the_coordinate_system() {
        let dir = std::env::temp_dir().join(format!("gis-json-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fc = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[10.5,47.1]}},
            {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[10,47],[10.1,47.1],[10.2,47.05]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]],[[0.2,0.2],[0.2,0.4],[0.4,0.4],[0.4,0.2],[0.2,0.2]]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"MultiPolygon","coordinates":[[[[2,2],[3,2],[3,3],[2,3],[2,2]]],[[[4,4],[5,4],[5,5],[4,5],[4,4]]]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,1]},{"type":"MultiPoint","coordinates":[[2,2],[3,3]]}]}},
            {"type":"Feature","properties":{},"geometry":null}
        ]}"#;
        std::fs::write(dir.join("a.geojson"), fc).unwrap();
        let l = read_vector(&dir.join("a.geojson"), None).unwrap().remove(0);
        assert_eq!((l.data.points.len(), l.data.lines.len(), l.data.polys.len(), l.data.polys[0].len()), (4, 1, 3, 2));
        assert_eq!(l.crs, Some(Crs::LonLat));
        // the old "crs" member
        let utm = r#"{"type":"Feature","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::32633"}},"geometry":{"type":"Point","coordinates":[412345,4851234]}}"#;
        std::fs::write(dir.join("b.json"), utm).unwrap();
        assert_eq!(read_vector(&dir.join("b.json"), None).unwrap()[0].crs, Some(Crs::Utm { zone: 33, north: true }));
        let odd = r#"{"type":"Feature","crs":{"properties":{"name":"EPSG:2"}},"geometry":{"type":"Point","coordinates":[1,2]}}"#;
        std::fs::write(dir.join("c.geojson"), odd).unwrap();
        assert_eq!(read_vector(&dir.join("c.geojson"), None).unwrap()[0].crs, None);
        // empty and invalid files are errors with the file's name
        std::fs::write(dir.join("d.geojson"), r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        assert!(read_vector(&dir.join("d.geojson"), None).is_err());
        std::fs::write(dir.join("e.geojson"), "not json").unwrap();
        assert!(read_vector(&dir.join("e.geojson"), None).unwrap_err().contains("e.geojson"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_geopackage_with_two_feature_tables_is_read_read_only() {
        let dir = std::env::temp_dir().join(format!("gis-gpkg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gpkg");
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER, definition TEXT);
                 INSERT INTO gpkg_spatial_ref_sys VALUES ('WGS 84 / UTM 33N', 32633, 'EPSG', 32633, 'x'), ('undefined', 0, 'NONE', 0, 'undefined');
                 CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT);
                 INSERT INTO gpkg_contents VALUES ('faults', 'features', 'faults'), ('units', 'features', 'units'), ('notes', 'attributes', 'notes');
                 CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z INTEGER, m INTEGER);
                 INSERT INTO gpkg_geometry_columns VALUES ('faults', 'geom', 'LINESTRING', 32633, 0, 0), ('units', 'geom', 'POLYGON', 0, 0, 0);
                 CREATE TABLE faults (fid INTEGER PRIMARY KEY, geom BLOB, name TEXT);
                 CREATE TABLE units (fid INTEGER PRIMARY KEY, geom BLOB);",
            )
            .unwrap();
            let blob = |wkb: Vec<u8>, srs: i32| {
                let mut b = vec![b'G', b'P', 0, 1];
                b.extend(srs.to_le_bytes());
                b.extend(wkb);
                b
            };
            let mut line = vec![1u8, 2, 0, 0, 0, 2, 0, 0, 0];
            for p in [[1.0f64, 2.0], [3.0, 4.0]] {
                line.extend(p[0].to_le_bytes());
                line.extend(p[1].to_le_bytes());
            }
            db.execute("INSERT INTO faults (geom, name) VALUES (?1, 'F1')", [blob(line, 32633)]).unwrap();
            db.execute("INSERT INTO faults (geom, name) VALUES (NULL, 'F2')", []).unwrap();
            db.execute("INSERT INTO units (geom) VALUES (?1)", [blob(wkb_polygon_le(&[&SQUARE], 3), 0)]).unwrap();
        }
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let layers = read_vector(&path, None).unwrap();
        assert_eq!(layers.len(), 2);
        let f = layers.iter().find(|l| l.table == "faults").unwrap();
        assert_eq!((f.data.lines.len(), f.crs, f.name.as_str()), (1, Some(Crs::Utm { zone: 33, north: true }), "m - faults"));
        // undefined reference system: the user is asked
        let u = layers.iter().find(|l| l.table == "units").unwrap();
        assert_eq!((u.data.polys.len(), u.crs), (1, None));
        // one table wanted
        assert_eq!(read_vector(&path, Some("faults")).unwrap().len(), 1);
        assert!(read_vector(&path, Some("nope")).is_err());
        // the file was only read
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
        // not a GeoPackage
        std::fs::write(dir.join("x.gpkg"), b"SQLite format 3\0 nonsense").unwrap();
        assert!(read_vector(&dir.join("x.gpkg"), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coordinate_systems_survive_the_preferences_file() {
        for c in [Crs::LonLat, Crs::WebMerc, Crs::Utm { zone: 33, north: true }, Crs::Utm { zone: 7, north: false }, Crs::Epsg(2056)] {
            let t = crs_token(c).unwrap();
            assert_eq!(crs_from_token(&t), Some(c), "{t}");
        }
        assert_eq!(crs_token(Crs::Custom(0)), None);
        for bad in ["", "utm:", "utm:61n", "utm:33x", "epsg:", "epsg:abc", "nonsense"] {
            assert_eq!(crs_from_token(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_polygon_with_a_hole_is_cut_into_triangles_in_web_mercator() {
        let data = VectorData { points: vec![[0.0, 0.0]], lines: vec![vec![[0.0, 0.0], [1.0, 1.0]]], polys: vec![vec![SQUARE.to_vec(), HOLE.to_vec()]], ..Default::default() };
        let p = project(&data, Crs::WebMerc);
        assert_eq!((p.points.len(), p.lines.len(), p.polys.len()), (1, 1, 1));
        let poly = &p.polys[0];
        // the closing vertices are dropped: 4 + 4 vertices, two rings; 8 triangles for a square with a square hole
        assert_eq!((poly.verts.len(), poly.rings.len(), poly.tris.len()), (8, 2, 24));
        assert!(poly.tris.iter().all(|i| (*i as usize) < poly.verts.len()));
        // the triangles cover the area of the square minus the hole: 100 - 16
        let area: f64 = poly.tris.chunks(3).map(|t| {
            let (a, b, c) = (poly.verts[t[0] as usize], poly.verts[t[1] as usize], poly.verts[t[2] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
        }).sum();
        assert!((area - 84.0).abs() < 1e-9, "{area}");
        assert_eq!(p.bbox, Some(([0.0, 0.0], [10.0, 10.0])));
        // a geographic polygon lands where Web Mercator puts it
        let g = project(&VectorData { polys: vec![vec![vec![[10.0, 47.0], [10.1, 47.0], [10.1, 47.1], [10.0, 47.1]]]], ..Default::default() }, Crs::LonLat);
        let (mx, my) = ams_core::gis::merc_from_lonlat(10.0, 47.0);
        assert!((g.polys[0].verts[0][0] - mx).abs() < 1e-6 && (g.polys[0].verts[0][1] - my).abs() < 1e-6);
        assert!(project(&VectorData::default(), Crs::LonLat).bbox.is_none());
    }

    #[test]
    fn geojson_attributes_follow_their_features_even_when_a_feature_has_several_shapes() {
        let dir = std::env::temp_dir().join(format!("gis-gj-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.geojson");
        std::fs::write(&p, r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"HOLEID":"A1","DEPTH":12.5},"geometry":{"type":"Point","coordinates":[10.0,47.0]}},
          {"type":"Feature","properties":{"HOLEID":"B2","NOTE":null},"geometry":{"type":"MultiPoint","coordinates":[[10.1,47.1],[10.2,47.2]]}},
          {"type":"Feature","properties":{"HOLEID":"C3","DEPTH":7},"geometry":{"type":"Point","coordinates":[10.3,47.3]}}]}"#)
            .unwrap();
        let l = read_vector(&p, None).unwrap().remove(0);
        let d = &l.data;
        assert_eq!(d.fields, vec!["HOLEID", "DEPTH", "NOTE"]);
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[0], vec![AttrVal::Text("A1".into()), AttrVal::Num(12.5), AttrVal::Null]);
        // the multipoint's two points both belong to the second feature
        assert_eq!(d.pfeat, vec![0, 1, 1, 2]);
        let only_b = d.filtered(&[false, true, false]);
        assert_eq!((only_b.points.len(), only_b.pfeat.clone()), (2, vec![1, 1]));
        // without attributes (or a mask of the wrong length) nothing is dropped
        assert_eq!(d.filtered(&[true]).points.len(), 4);
        assert_eq!(project(&only_b, Crs::LonLat).pfeat, vec![1, 1]);
        assert_eq!(AttrVal::Num(7.0).text(), "7");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dbf_table_is_read_in_record_order() {
        // two fields: NAME (text, 6) and DEPTH (numeric, 6); the second record is deleted
        let mut b = vec![3u8, 124, 1, 1, 3, 0, 0, 0];
        b.extend(((32 + 2 * 32 + 1) as u16).to_le_bytes());
        b.extend((1 + 6 + 6u16).to_le_bytes());
        b.extend([0u8; 20]);
        for (name, ty) in [("NAME", b'C'), ("DEPTH", b'N')] {
            let mut f = [0u8; 32];
            f[..name.len()].copy_from_slice(name.as_bytes());
            f[11] = ty;
            f[16] = 6;
            b.extend(f);
        }
        b.push(0x0d);
        b.extend(b" PRB001  12.5");
        b.extend(b"*PRB002   9.0");
        b.extend(b" zk9         ");
        let dir = std::env::temp_dir().join(format!("gis-dbf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.dbf");
        std::fs::write(&p, &b).unwrap();
        let (fields, rows) = read_dbf(&p).unwrap();
        assert_eq!(fields, vec!["NAME", "DEPTH"]);
        assert_eq!(rows[0], vec![AttrVal::Text("PRB001".into()), AttrVal::Num(12.5)]);
        assert_eq!(rows[1], vec![AttrVal::Null, AttrVal::Null]);
        assert_eq!(rows[2], vec![AttrVal::Text("zk9".into()), AttrVal::Null]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
