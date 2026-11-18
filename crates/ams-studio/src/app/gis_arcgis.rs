//! An ArcGIS project as a layer tree for the GIS viewer, the same way `gis_qgis` does it for QGIS: only the *legend* and
//! the *paths* are read (groups, order, ticks, names, transparency, the **definition query** as the layer's filter, the
//! label field, the map's coordinate system); no data file is opened and styles are not carried over.
//!
//! * ArcGIS Pro: `.aprx` (a zip of JSON documents), `.lyrx` (layer file) and `.mapx` (map file). All three hold CIM
//!   JSON: layer definitions (`CIMFeatureLayer`, `CIMRasterLayer`, `CIMGroupLayer`, ...) that a map or a layer document
//!   lists by URI. Every JSON document found is read, so the exact file layout of the archive does not matter.
//! * ArcMap (older): `.mxd` and `.lyr`, binary compound documents. Their layer records are not readable, but the data
//!   paths and layer names are stored as text, so the layers are found by those paths. Ticks, groups, definition
//!   queries and labels are not recoverable there: vector layers are ticked, rasters are not, and layers that share a
//!   folder are grouped by it.
//!
//! What cannot be shown is left out and listed with its reason: online layers and basemaps, file geodatabases and
//! other databases, layers whose file is not there. Tested without a window on made-up documents.

use super::gis_qgis::{Node, Project, QLayer};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Whether a file name is an ArcGIS project or layer file this module reads.
pub fn is_arcgis(path: &Path) -> bool {
    matches!(path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("aprx" | "lyrx" | "mapx" | "mxd" | "lyr"))
}

/// Reads an ArcGIS project or layer file.
pub fn read_project(path: &Path) -> Result<Project, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let base = path.parent().unwrap_or(Path::new("."));
    let fs = Fs { exists: &|p| p.exists(), is_dir: &|p| p.is_dir(), find: &|b, name| find_by_name(b, name) };
    match ext.as_str() {
        "aprx" => {
            use std::io::Read;
            let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| format!("Not an ArcGIS Pro project (.aprx is a zip archive): {e}"))?;
            let names: Vec<String> = z.file_names().filter_map(|n| n.ok().map(|n| n.to_string())).collect();
            let mut docs: Vec<Value> = vec![];
            for n in names.iter().filter(|n| n.to_lowercase().ends_with(".json")) {
                let mut s = String::new();
                if z.by_name(n).map_err(|e| e.to_string())?.read_to_string(&mut s).is_ok() {
                    if let Ok(v) = serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')) {
                        docs.push(v);
                    }
                }
            }
            if docs.is_empty() {
                return Err("This .aprx keeps its maps in an older XML form that AMS Studio does not read. Open it in ArcGIS Pro and use Share > Save As Layer File (.lyrx) or Save As Map File (.mapx), then add that file here.".into());
            }
            parse_cim(&docs, base, &fs)
        }
        "lyrx" | "mapx" => {
            let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let v: Value = serde_json::from_str(s.trim_start_matches('\u{feff}')).map_err(|e| format!("The file is damaged (not JSON): {e}"))?;
            parse_cim(&[v], base, &fs)
        }
        "mxd" | "lyr" => {
            let strings = legacy_strings(path)?;
            let list_dir = |d: &Path| -> Vec<String> { std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default() };
            parse_legacy(&strings, base, &fs, &list_dir)
        }
        _ => Err("Not an ArcGIS file (.aprx, .lyrx, .mapx, .mxd or .lyr).".into()),
    }
}

/// What the readers need from the disk (parameters so that the tests need no files).
pub struct Fs<'a> {
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub is_dir: &'a dyn Fn(&Path) -> bool,
    /// A file of this name somewhere below the folder (the project was moved with its data).
    pub find: &'a dyn Fn(&Path, &str) -> Option<PathBuf>,
}

fn find_by_name(base: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_lowercase();
    let mut budget = 6000usize;
    let mut stack = vec![(base.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            if budget == 0 {
                return None;
            }
            budget -= 1;
            let p = e.path();
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if e.file_name().to_string_lossy().to_lowercase() == want {
                return Some(p);
            }
        }
    }
    None
}

/// `C:\data\x` or `..\x` or `x/y` as a path of this system.
fn norm(s: &str) -> PathBuf {
    PathBuf::from(s.trim().replace('\\', "/"))
}

/// The path of a file the document names. A relative path starts at the document's folder. A path of another computer
/// (`C:\Users\someone\project\data\x.shp`) is looked for by its tail below the document's folder and its parents
/// (`data/x.shp`), and finally by the file name alone.
pub fn resolve_file(original: &str, base: &Path, fs: &Fs) -> Option<PathBuf> {
    let p = norm(original);
    let direct = if p.is_absolute() { p.clone() } else { base.join(&p) };
    if (fs.exists)(&direct) {
        return Some(direct);
    }
    let comps: Vec<String> = original.replace('\\', "/").split('/').filter(|c| !c.is_empty() && !c.ends_with(':') && *c != "." && *c != "..").map(|c| c.to_string()).collect();
    for skip in 0..comps.len() {
        let tail: PathBuf = comps[skip..].iter().collect();
        for up in [base.to_path_buf(), base.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| base.to_path_buf())] {
            let cand = up.join(&tail);
            if (fs.exists)(&cand) {
                return Some(cand);
            }
        }
    }
    let name = comps.last()?;
    (fs.find)(base, name)
}

/// `[FIELD]` -> `"FIELD"` outside single-quoted text (the filter syntax of AMS Studio is the QGIS / SQL one).
pub fn brackets_to_quotes(s: &str) -> String {
    let mut out = String::new();
    let mut in_text = false;
    for c in s.chars() {
        match c {
            '\'' => {
                in_text = !in_text;
                out.push(c);
            }
            '[' if !in_text => out.push('"'),
            ']' if !in_text => out.push('"'),
            _ => out.push(c),
        }
    }
    out
}

/// The field a label expression is made of: `[NAME]`, `$feature.NAME`, `$feature["NAME"]` or `"NAME"` (the first one).
pub fn label_field(expr: &str) -> String {
    let e = expr.trim();
    if let Some(i) = e.find("$feature") {
        let rest = &e[i + 8..];
        if let Some(r) = rest.strip_prefix('.') {
            return r.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        }
        if let Some(r) = rest.strip_prefix("[\"").or_else(|| rest.strip_prefix("['")) {
            return r.chars().take_while(|c| *c != '"' && *c != '\'').collect();
        }
    }
    for (open, close) in [('[', ']'), ('"', '"')] {
        if let Some(a) = e.find(open) {
            if let Some(b) = e[a + 1..].find(close) {
                let f = &e[a + 1..a + 1 + b];
                if !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return f.to_string();
                }
            }
        }
    }
    String::new()
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

fn uri_of(v: &Value) -> Option<&str> {
    s(v, "uRI").or_else(|| s(v, "uri")).or_else(|| s(v, "URI"))
}

/// `DATABASE=...` out of a workspace connection string (`KEY=value;KEY=value`).
fn database_of(conn: &str) -> Option<String> {
    conn.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        k.trim().eq_ignore_ascii_case("DATABASE").then(|| v.trim().to_string())
    })
}

/// The data connection of a layer, following a join to the table it is made from.
fn connection<'a>(def: &'a Value) -> Option<&'a Value> {
    let mut c = def.get("featureTable").and_then(|t| t.get("dataConnection")).or_else(|| def.get("dataConnection"))?;
    for _ in 0..4 {
        match c.get("sourceTable").and_then(|t| t.get("dataConnection").or(Some(t))) {
            Some(inner) if inner.get("workspaceConnectionString").is_some() || inner.get("sourceTable").is_some() || inner.get("dataset").is_some() => c = inner,
            _ => break,
        }
    }
    Some(c)
}

/// What a connection points at: the file, and the table of a GeoPackage; or why it cannot be shown.
fn locate(def: &Value, raster: bool, base: &Path, fs: &Fs) -> Result<(PathBuf, Option<String>), String> {
    let c = connection(def).ok_or("no data source")?;
    let conn = s(c, "workspaceConnectionString").unwrap_or_default();
    let ws = database_of(conn).ok_or("no data folder in the layer")?;
    let dataset = s(c, "dataset").unwrap_or_default();
    let wl = ws.to_lowercase();
    if wl.ends_with(".gdb") {
        return Err("file geodatabase: export the layer to a shapefile or GeoPackage".into());
    }
    if wl.ends_with(".sde") || wl.ends_with(".mdb") || wl.ends_with(".accdb") || s(c, "workspaceFactory").is_some_and(|f| f.eq_ignore_ascii_case("SDE")) {
        return Err("database connection".into());
    }
    if wl.ends_with(".gpkg") {
        let table = dataset.rsplit('.').next().unwrap_or(dataset).to_string();
        let p = resolve_file(&ws, base, fs).ok_or("file not found")?;
        return Ok((p, (!table.is_empty()).then_some(table)));
    }
    if dataset.is_empty() {
        return Err("no dataset name".into());
    }
    let exts: &[&str] = if raster { &["", ".tif", ".tiff", ".png", ".jpg", ".jpeg"] } else { &["", ".shp", ".geojson", ".json"] };
    let folder = ws.trim_end_matches(['\\', '/']).to_string();
    for e in exts {
        let name = if e.is_empty() || dataset.to_lowercase().ends_with(e) { dataset.to_string() } else { format!("{dataset}{e}") };
        if name.rsplit_once('.').is_none() {
            continue; // an Esri grid or a table without extension
        }
        if let Some(p) = resolve_file(&format!("{folder}/{name}"), base, fs) {
            return Ok((p, None));
        }
    }
    Err("file not found".into())
}

/// A coordinate system of a CIM `spatialReference` as `epsg:N`.
fn wkid_of(v: &Value) -> Option<String> {
    let sr = v.get("spatialReference")?;
    let w = sr.get("latestWkid").or_else(|| sr.get("wkid")).and_then(|w| w.as_u64())?;
    (w > 0 && w < 1_000_000).then(|| format!("epsg:{w}"))
}

struct Cim<'a> {
    defs: Vec<&'a Value>,
    maps: Vec<&'a Value>,
    roots: Vec<&'a Value>,
}

fn collect<'a>(v: &'a Value, out: &mut Cim<'a>) {
    match v {
        Value::Object(o) => {
            if let Some(t) = o.get("type").and_then(|t| t.as_str()) {
                if t.starts_with("CIM") && t.ends_with("Layer") {
                    out.defs.push(v);
                } else if t == "CIMMap" {
                    out.maps.push(v);
                } else if t == "CIMLayerDocument" || t == "CIMMapDocument" {
                    out.roots.push(v);
                }
            }
            for x in o.values() {
                collect(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        _ => {}
    }
}

/// Parses CIM JSON documents (the documents of an `.aprx`, or a `.lyrx` / `.mapx`). `base` is the folder relative paths
/// start from.
pub fn parse_cim(docs: &[Value], base: &Path, fs: &Fs) -> Result<Project, String> {
    let mut c = Cim { defs: vec![], maps: vec![], roots: vec![] };
    for d in docs {
        collect(d, &mut c);
    }
    // one definition per URI (an archive may repeat them); a definition without a URI is kept as it comes
    let mut defs: Vec<&Value> = vec![];
    let mut by_uri: std::collections::HashMap<String, usize> = Default::default();
    for d in &c.defs {
        match uri_of(d) {
            Some(u) => {
                if !by_uri.contains_key(u) {
                    by_uri.insert(u.to_string(), defs.len());
                    defs.push(d);
                }
            }
            None => defs.push(d),
        }
    }
    if defs.is_empty() {
        return Err("The file holds no layer definitions. If it is a project saved by a very old version of ArcGIS Pro, save a layer file (.lyrx) or a map file (.mapx) from it.".into());
    }
    let lookup = |u: &str| -> Option<usize> {
        by_uri.get(u).copied().or_else(|| {
            // the same layer under a different folder prefix: compare the last part of the URI
            let tail = u.rsplit('/').next().unwrap_or(u);
            by_uri.iter().find(|(k, _)| k.rsplit('/').next() == Some(tail)).map(|(_, i)| *i)
        })
    };
    let crs = c.maps.iter().find_map(|m| wkid_of(m)).or_else(|| docs.iter().find_map(wkid_of));
    let mut proj = Project { source: "ArcGIS", ..Default::default() };
    let mut used = vec![false; defs.len()];

    fn node(i: usize, defs: &[&Value], lookup: &dyn Fn(&str) -> Option<usize>, used: &mut Vec<bool>, depth: usize, crs: &Option<String>, base: &Path, fs: &Fs, proj: &mut Project) -> Option<Node> {
        let d = defs[i];
        used[i] = true;
        let ty = s(d, "type").unwrap_or_default();
        let name = s(d, "name").unwrap_or("Layer").to_string();
        let checked = d.get("visibility").and_then(|v| v.as_bool()).unwrap_or(true);
        if ty == "CIMGroupLayer" {
            if depth > 16 {
                return None;
            }
            let mut kids = vec![];
            for u in d.get("layers").and_then(|l| l.as_array()).into_iter().flatten().filter_map(|u| u.as_str()) {
                if let Some(j) = lookup(u) {
                    if let Some(n) = node(j, defs, lookup, used, depth + 1, crs, base, fs, proj) {
                        kids.push(n);
                    }
                }
            }
            if kids.is_empty() {
                proj.empty_groups += 1;
                return None;
            }
            return Some(Node::Group { name, checked, open: d.get("expanded").and_then(|v| v.as_bool()).unwrap_or(true), children: kids });
        }
        let raster = matches!(ty, "CIMRasterLayer" | "CIMMosaicLayer");
        if !raster && ty != "CIMFeatureLayer" {
            let why = match ty {
                "CIMTiledServiceLayer" | "CIMWMSLayer" | "CIMVectorTileLayer" | "CIMImageServiceLayer" | "CIMBasemapLayer" | "CIMVectorTileLayerBase" => "online layer or basemap (the backdrop is built in)".to_string(),
                "CIMAnnotationLayer" => "annotation".to_string(),
                "CIMGraphicsLayer" => "graphics".to_string(),
                other => format!("layer type {} is not shown", other.trim_start_matches("CIM")),
            };
            proj.skipped.push((name, why));
            return None;
        }
        if s(d, "layerType").is_some_and(|t| t.to_lowercase().contains("basemap")) {
            proj.skipped.push((name, "basemap (the backdrop is built in)".into()));
            return None;
        }
        match locate(d, raster, base, fs) {
            Ok((path, table)) => {
                let mut filter = String::new();
                if !raster {
                    if let Some(q) = d.get("featureTable").and_then(|t| s(t, "definitionExpression")).filter(|q| !q.trim().is_empty()) {
                        let kind = d.get("featureTable").and_then(|t| s(t, "definitionExpressionType")).unwrap_or("SQL");
                        if kind.eq_ignore_ascii_case("SQL") {
                            filter = brackets_to_quotes(q.trim());
                        } else {
                            proj.skipped.push((format!("{name} (filter)"), format!("the filter is written in {kind}, not SQL; the layer is added without it")));
                        }
                    }
                }
                let label = if !raster && d.get("labelVisibility").and_then(|v| v.as_bool()).unwrap_or(false) {
                    d.get("labelClasses").and_then(|l| l.as_array()).and_then(|l| l.first()).and_then(|l| s(l, "expression")).map(label_field).unwrap_or_default()
                } else {
                    String::new()
                };
                let transparency = d.get("transparency").and_then(|t| t.as_f64()).unwrap_or(0.0).clamp(0.0, 95.0);
                Some(Node::Layer(QLayer { name, path, table, raster, checked, opacity: (1.0 - transparency / 100.0) as f32, filter, label, crs: crs.clone() }))
            }
            Err(why) => {
                proj.skipped.push((name, why));
                None
            }
        }
    }

    let top_uris = |v: &Value| -> Vec<String> { v.get("layers").and_then(|l| l.as_array()).map(|a| a.iter().filter_map(|u| u.as_str().map(|u| u.to_string())).collect()).unwrap_or_default() };
    let mut tops: Vec<(Option<String>, Vec<String>)> = vec![];
    if !c.maps.is_empty() {
        for m in &c.maps {
            tops.push((s(m, "name").map(|n| n.to_string()), top_uris(m)));
        }
    } else {
        for r in &c.roots {
            tops.push((None, top_uris(r)));
        }
    }
    let many_maps = tops.iter().filter(|t| !t.1.is_empty()).count() > 1;
    for (map_name, uris) in &tops {
        let mut nodes = vec![];
        for u in uris {
            if let Some(i) = lookup(u) {
                if let Some(n) = node(i, &defs, &lookup, &mut used, 0, &crs, base, fs, &mut proj) {
                    nodes.push(n);
                }
            }
        }
        if many_maps && !nodes.is_empty() {
            proj.nodes.push(Node::Group { name: map_name.clone().unwrap_or_else(|| "Map".into()), checked: true, open: true, children: nodes });
        } else {
            proj.nodes.extend(nodes);
        }
    }
    // layers no map or document listed (a layer file that lists none): in the order they come, groups first resolved
    let referenced_by_group: std::collections::HashSet<usize> = defs
        .iter()
        .filter(|d| s(d, "type") == Some("CIMGroupLayer"))
        .flat_map(|d| d.get("layers").and_then(|l| l.as_array()).into_iter().flatten().filter_map(|u| u.as_str()).filter_map(|u| lookup(u)).collect::<Vec<_>>())
        .collect();
    if proj.nodes.is_empty() && proj.skipped.is_empty() {
        for i in 0..defs.len() {
            if !used[i] && !referenced_by_group.contains(&i) {
                if let Some(n) = node(i, &defs, &lookup, &mut used, 0, &crs, base, fs, &mut proj) {
                    proj.nodes.push(n);
                }
            }
        }
    }
    Ok(proj)
}

// ------------------------------------------------------------------------------------------------ ArcMap (.mxd, .lyr)

/// Every text run of at least three characters stored as UTF-16 in the compound document, in file order.
pub fn legacy_strings(path: &Path) -> Result<Vec<String>, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cf = cfb::CompoundFile::open(std::io::BufReader::new(f)).map_err(|e| format!("Not an ArcMap document: {e}"))?;
    let streams: Vec<String> = cf.walk().filter(|e| e.is_stream()).map(|e| e.path().to_string_lossy().to_string()).collect();
    let mut out = vec![];
    for name in streams {
        // the big tool and toolbox streams hold no layers
        if name.contains("ArcToolbox") || name.contains("StyleGallery") {
            continue;
        }
        let mut bytes = vec![];
        if let Ok(mut st) = cf.open_stream(&name) {
            let _ = st.read_to_end(&mut bytes);
        }
        out.extend(utf16_runs(&bytes));
    }
    Ok(out)
}

/// UTF-16LE runs of printable ASCII of at least three characters, at any alignment.
pub fn utf16_runs(b: &[u8]) -> Vec<String> {
    let mut out = vec![];
    let mut i = 0;
    while i + 1 < b.len() {
        if (0x20..0x7f).contains(&b[i]) && b[i + 1] == 0 {
            let mut s = String::new();
            let mut j = i;
            while j + 1 < b.len() && (0x20..0x7f).contains(&b[j]) && b[j + 1] == 0 {
                s.push(b[j] as char);
                j += 2;
            }
            if s.len() >= 3 {
                out.push(s);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

const VECTOR_EXT: &[&str] = &["shp", "geojson"];
const RASTER_EXT: &[&str] = &["tif", "tiff", "png", "jpg", "jpeg"];

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

/// Finds the layers of an ArcMap document from its text: `RASTER: Workspace = <folder>; RasterDataset = <file>;` for a
/// raster, and for a vector layer a folder path followed or preceded by the name of a shapefile in it.
pub fn parse_legacy(strings: &[String], base: &Path, fs: &Fs, list_dir: &dyn Fn(&Path) -> Vec<String>) -> Result<Project, String> {
    let mut found: Vec<(PathBuf, bool)> = vec![];
    let push = |p: PathBuf, raster: bool, found: &mut Vec<(PathBuf, bool)>| {
        if !found.iter().any(|(q, _)| q.to_string_lossy().to_lowercase() == p.to_string_lossy().to_lowercase()) {
            found.push((p, raster));
        }
    };
    let mut gdb = 0usize;
    let mut missing: Vec<String> = vec![];
    for (k, s) in strings.iter().enumerate() {
        if let Some(rest) = s.strip_prefix("RASTER:") {
            // RASTER: Workspace = C:\x\; RasterDataset = a.tif;
            let ws = rest.split("Workspace =").nth(1).and_then(|r| r.split(';').next()).map(|w| w.trim().to_string());
            let ds = rest.split("RasterDataset =").nth(1).and_then(|r| r.split(';').next()).map(|w| w.trim().to_string());
            if let (Some(ws), Some(ds)) = (ws, ds) {
                let full = format!("{}/{}", ws.trim_end_matches(['\\', '/']), ds);
                match resolve_file(&full, base, fs) {
                    Some(p) => push(p, true, &mut found),
                    None => missing.push(ds),
                }
            }
            continue;
        }
        if s.eq_ignore_ascii_case("File Geodatabase Feature Class") {
            gdb += 1;
            continue;
        }
        // a folder path (not a sentence, not a whole file): the layers of this folder named next to it
        let pathlike = s.len() < 260 && (s.contains('\\') || s.contains('/')) && !s.contains(';') && !s.contains(" - ") && !s.to_lowercase().ends_with(".style") && !s.starts_with("\\\\") && s.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '.');
        if !pathlike {
            continue;
        }
        let dir = resolve_file(s, base, &Fs { exists: fs.is_dir, is_dir: fs.is_dir, find: &|_, _| None });
        let Some(dir) = dir else { continue };
        let files = list_dir(&dir);
        let near: Vec<&String> = (k.saturating_sub(4)..(k + 4).min(strings.len())).filter(|i| *i != k).map(|i| &strings[i]).collect();
        for name in near {
            let want = name.to_lowercase();
            if let Some(f) = files.iter().find(|f| {
                let (st, ex) = (f.rsplit_once('.').map(|x| x.0).unwrap_or(f).to_lowercase(), ext_of(f));
                st == want && VECTOR_EXT.contains(&ex.as_str())
            }) {
                push(dir.join(f), false, &mut found);
            }
        }
    }
    // a layer file (.lyr) names its data without a folder: the data lies beside it
    let local = list_dir(base);
    let mut seen_names: std::collections::HashSet<String> = Default::default();
    for name in strings {
        let want = name.to_lowercase();
        if !seen_names.insert(want.clone()) || want.len() < 3 {
            continue;
        }
        for f in &local {
            let (st, ex) = (f.rsplit_once('.').map(|x| x.0).unwrap_or(f).to_lowercase(), ext_of(f));
            if st == want && VECTOR_EXT.contains(&ex.as_str()) {
                push(base.join(f), false, &mut found);
            } else if st == want && RASTER_EXT.contains(&ex.as_str()) {
                push(base.join(f), true, &mut found);
            }
        }
    }
    if found.is_empty() {
        return Err(format!("No layer with a readable file path was found in this ArcMap document.{}", if gdb > 0 { " Its layers are in a file geodatabase, which AMS Studio cannot read: export them to shapefiles or a GeoPackage." } else { "" }));
    }
    let mut proj = Project { source: "ArcGIS", ..Default::default() };
    if gdb > 0 {
        proj.skipped.push((format!("file geodatabase layers (n={gdb})"), "file geodatabase: export to shapefile or GeoPackage".into()));
    }
    for m in missing.iter().take(5) {
        proj.skipped.push((m.clone(), "file not found".into()));
    }
    proj.note = "An ArcMap document (.mxd, .lyr) keeps no readable ticks, groups, filters or labels, so vector layers are ticked, rasters are not (tick them to load), and layers of one folder are grouped by it. Save the map from ArcGIS Pro as .mapx for the full legend.".to_string();
    // layers that share a folder (three or more) go into a group named after it
    let dir_of = |p: &Path| p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let mut order: Vec<(PathBuf, Vec<usize>)> = vec![];
    for (i, (p, _)) in found.iter().enumerate() {
        let d = dir_of(p);
        match order.iter_mut().find(|o| o.0 == d) {
            Some(o) => o.1.push(i),
            None => order.push((d, vec![i])),
        }
    }
    let layer = |i: usize| -> Node {
        let (p, raster) = &found[i];
        Node::Layer(QLayer { name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), path: p.clone(), table: None, raster: *raster, checked: !raster, opacity: 1.0, filter: String::new(), label: String::new(), crs: None })
    };
    for (d, idx) in order {
        if idx.len() >= 3 {
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Layers".into());
            let any_on = idx.iter().any(|&i| !found[i].1);
            proj.nodes.push(Node::Group { name, checked: any_on, open: false, children: idx.into_iter().map(layer).collect() });
        } else {
            proj.nodes.extend(idx.into_iter().map(layer));
        }
    }
    Ok(proj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all_exist() -> (Box<dyn Fn(&Path) -> bool>, Box<dyn Fn(&Path, &str) -> Option<PathBuf>>) {
        (Box::new(|_| true), Box::new(|_, _| None))
    }

    fn feature(name: &str, uri: &str, ws: &str, ds: &str, extra: Value) -> Value {
        let mut v = json!({"type": "CIMFeatureLayer", "name": name, "uRI": uri, "visibility": true,
            "featureTable": {"type": "CIMFeatureTable", "dataConnection": {"type": "CIMStandardDataConnection", "workspaceConnectionString": format!("DATABASE={ws}"), "workspaceFactory": "Shapefile", "dataset": ds, "datasetType": "esriDTFeatureClass"}}});
        if let Some(o) = extra.as_object() {
            for (k, x) in o {
                if k == "featureTable" {
                    for (k2, x2) in x.as_object().unwrap() {
                        v["featureTable"][k2] = x2.clone();
                    }
                } else {
                    v[k] = x.clone();
                }
            }
        }
        v
    }

    #[test]
    fn a_map_file_gives_groups_filters_labels_and_transparency() {
        let doc = json!({"type": "CIMMapDocument", "mapDefinition": {"type": "CIMMap", "name": "Field map", "spatialReference": {"wkid": 27700, "latestWkid": 27700},
            "layers": ["CIMPATH=m/geo.json", "CIMPATH=m/dem.json", "CIMPATH=m/web.json", "CIMPATH=m/gdb.json"]},
            "layerDefinitions": [
              {"type": "CIMGroupLayer", "name": "Geology", "uRI": "CIMPATH=m/geo.json", "visibility": false, "expanded": true, "layers": ["CIMPATH=m/faults.json", "CIMPATH=m/units.json"]},
              feature("Faults", "CIMPATH=m/faults.json", "C:\\Users\\other\\gis\\data", "faults.shp", json!({"visibility": true, "transparency": 40, "labelVisibility": true, "labelClasses": [{"expression": "$feature.TYPE"}]})),
              feature("Units", "CIMPATH=m/units.json", "data", "units", json!({"featureTable": {"definitionExpression": "[AGE] = 'Silurian' AND [DIP] > 20", "definitionExpressionType": "SQL"}})),
              {"type": "CIMRasterLayer", "name": "DEM", "uRI": "CIMPATH=m/dem.json", "visibility": true, "transparency": 10, "dataConnection": {"type": "CIMStandardDataConnection", "workspaceConnectionString": "DATABASE=data", "workspaceFactory": "Raster", "dataset": "dem.tif"}},
              {"type": "CIMTiledServiceLayer", "name": "World imagery", "uRI": "CIMPATH=m/web.json"},
              feature("Stations", "CIMPATH=m/gdb.json", "C:\\x\\Geodatabase.gdb", "stations", json!({}))
            ]});
        let exists = |p: &Path| matches!(p.to_string_lossy().replace('\\', "/").as_str(), "/proj/data/faults.shp" | "/proj/data/units.shp" | "/proj/data/dem.tif");
        let find = |_: &Path, _: &str| None;
        let p = parse_cim(&[doc], Path::new("/proj"), &Fs { exists: &exists, is_dir: &exists, find: &find }).unwrap();
        assert_eq!(p.layer_count(), 3, "{:?}", p.skipped);
        let Node::Group { name, checked, children, .. } = &p.nodes[0] else { panic!("{:?}", p.nodes) };
        assert_eq!((name.as_str(), *checked, children.len()), ("Geology", false, 2));
        let Node::Layer(f) = &children[0] else { panic!() };
        // a path of another computer is found by its tail below the project folder
        assert_eq!((f.path.as_path(), f.label.as_str(), f.crs.as_deref()), (Path::new("/proj/data/faults.shp"), "TYPE", Some("epsg:27700")));
        assert!((f.opacity - 0.6).abs() < 1e-6);
        let Node::Layer(u) = &children[1] else { panic!() };
        assert_eq!(u.filter, "\"AGE\" = 'Silurian' AND \"DIP\" > 20");
        assert!(matches!(&p.nodes[1], Node::Layer(d) if d.raster && d.name == "DEM" && (d.opacity - 0.9).abs() < 1e-6));
        let why: Vec<&str> = p.skipped.iter().map(|(_, w)| w.as_str()).collect();
        assert!(why.iter().any(|w| w.contains("online")) && why.iter().any(|w| w.contains("geodatabase")), "{why:?}");
        assert!(p.summary().contains("ArcGIS"));
    }

    #[test]
    fn several_maps_become_groups_and_a_layer_file_lists_its_layers() {
        let (e, f) = all_exist();
        let fs = Fs { exists: &*e, is_dir: &*e, find: &*f };
        let two = json!({"type": "CIMProject", "maps": [
            {"type": "CIMMap", "name": "A", "layers": ["u1"]}, {"type": "CIMMap", "name": "B", "layers": ["u2"]}],
            "defs": [feature("One", "u1", "d", "one.shp", json!({})), feature("Two", "u2", "d", "two.shp", json!({}))]});
        let p = parse_cim(&[two], Path::new("/p"), &fs).unwrap();
        assert!(matches!(&p.nodes[0], Node::Group { name, .. } if name == "A") && matches!(&p.nodes[1], Node::Group { name, .. } if name == "B"));
        let lyrx = json!({"type": "CIMLayerDocument", "layers": ["x1"], "layerDefinitions": [feature("Solo", "x1", "d", "solo.shp", json!({}))]});
        let p = parse_cim(&[lyrx], Path::new("/p"), &fs).unwrap();
        assert_eq!(p.layer_count(), 1);
        // a layer file that lists none still gives its layers
        let bare = json!({"type": "CIMLayerDocument", "layerDefinitions": [feature("Bare", "b1", "d", "bare.shp", json!({}))]});
        assert_eq!(parse_cim(&[bare], Path::new("/p"), &fs).unwrap().layer_count(), 1);
        assert!(parse_cim(&[json!({"type": "CIMProject"})], Path::new("/p"), &fs).is_err());
    }

    #[test]
    fn a_geopackage_table_and_a_join_are_followed() {
        let (e, f) = all_exist();
        let fs = Fs { exists: &*e, is_dir: &*e, find: &*f };
        let gpkg = json!({"type": "CIMLayerDocument", "layers": ["g"], "layerDefinitions": [{"type": "CIMFeatureLayer", "name": "Sites", "uRI": "g", "featureTable": {"dataConnection": {"type": "CIMRelQueryTableDataConnection", "sourceTable": {"type": "CIMStandardDataConnection", "workspaceConnectionString": "AUTHENTICATION_MODE=OSA;DATABASE=C:\\g\\sites.gpkg;DBCLIENT=sqlite", "dataset": "main.sites"}}}}]});
        let p = parse_cim(&[gpkg], Path::new("/p"), &fs).unwrap();
        let Node::Layer(l) = &p.nodes[0] else { panic!("{:?}", p.skipped) };
        assert_eq!(l.table.as_deref(), Some("sites"));
    }

    #[test]
    fn filter_and_label_helpers() {
        assert_eq!(brackets_to_quotes("[A] = 'x [y]' OR [B] > 1"), "\"A\" = 'x [y]' OR \"B\" > 1");
        assert_eq!(label_field("[NAME]"), "NAME");
        assert_eq!(label_field("$feature.Strike + '/' + $feature.Dip"), "Strike");
        assert_eq!(label_field("$feature[\"Lith Unit\"]"), "Lith Unit");
        assert_eq!(label_field("\"NAME\""), "NAME");
        assert_eq!(label_field("1 + 1"), "");
    }

    #[test]
    fn an_arcmap_document_is_read_by_its_paths() {
        let u16s = |s: &str| s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>();
        let mut bytes = vec![1u8, 2, 3];
        for s in ["Layers", "FAULTS_linear", "maps\\geo\\sc1", "FEATURE", "Units", "maps\\geo\\sc1", "RASTER: Workspace = C:\\Users\\old\\GIS\\maps\\ras\\; RasterDataset = tile1.tif;", "File Geodatabase Feature Class"] {
            bytes.extend(u16s(s));
            bytes.extend([0xff, 0x01, 0x07]);
        }
        let strings = utf16_runs(&bytes);
        assert!(strings.contains(&"FAULTS_linear".to_string()), "{strings:?}");
        let exists = |p: &Path| {
            let s = p.to_string_lossy().replace('\\', "/");
            matches!(s.as_str(), "/proj/maps/geo/sc1" | "/proj/maps/ras/tile1.tif" | "/proj/maps/ras")
        };
        let find = |_: &Path, _: &str| None;
        let list = |d: &Path| if d.ends_with("sc1") { vec!["faults_linear.shp".to_string(), "faults_linear.dbf".to_string(), "units.shp".to_string()] } else { vec![] };
        let p = parse_legacy(&strings, Path::new("/proj"), &Fs { exists: &exists, is_dir: &exists, find: &find }, &list).unwrap();
        let names: Vec<String> = p.rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, ["faults_linear", "units", "tile1"], "{:?}", p.skipped);
        let rows = p.rows();
        assert!(rows[0].visible && rows[1].visible && !rows[2].visible, "vectors ticked, rasters not");
        assert!(p.skipped.iter().any(|(_, w)| w.contains("geodatabase")));
        assert!(p.summary().contains("ArcMap"));
        // a layer file names only the data, which lies beside it
        let lyr = vec!["Layer".to_string(), "SC045E_bedrock".to_string()];
        let there = |p: &Path| p.to_string_lossy().replace('\\', "/") == "/lyr/sc045e_bedrock.shp";
        let ls = |_: &Path| vec!["sc045e_bedrock.shp".to_string(), "sc045e_bedrock.lyr".to_string()];
        let p = parse_legacy(&lyr, Path::new("/lyr"), &Fs { exists: &there, is_dir: &there, find: &find }, &ls).unwrap();
        assert_eq!(p.layer_count(), 1);
    }

    /// `AMS_STUDIO_TEST_ARCGIS=<file> cargo test a_real_arcgis -- --ignored --nocapture` prints what a real file gives.
    #[test]
    #[ignore]
    fn a_real_arcgis_file_can_be_read() {
        let Some(p) = std::env::var_os("AMS_STUDIO_TEST_ARCGIS") else { return };
        let proj = read_project(Path::new(&p)).unwrap();
        for r in proj.rows() {
            println!("{}{} [{}] {}", if r.parent.is_some() { "    " } else { "" }, r.name, if r.visible { "x" } else { " " }, r.layer.as_ref().map(|l| l.path.display().to_string()).unwrap_or_default());
        }
        println!("{}", proj.summary());
    }
}
