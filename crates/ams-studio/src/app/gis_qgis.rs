//! A QGIS project (`.qgz`, a zip with the project inside, or a bare `.qgs`) as a layer tree for the GIS viewer.
//!
//! Only the *legend* and the *paths* are read: the groups with their names, tick state and folding, the layers in
//! their order with their names, tick state and opacity, and where each layer's file is (relative paths are taken
//! from the folder of the project, the table of a GeoPackage from `layername=`), the **filter** of a layer (the SQL
//! condition QGIS keeps as `subset=` in the data source), the attribute it is **labelled** with, and its coordinate
//! system (the project's for a layer that has none, as QGIS assumes). No data file is opened here, and styles
//! (colours, colour ramps) are not carried over; the viewer gives layers its own look.
//!
//! What cannot be shown is left out and listed with its reason: online layers (WMS / XYZ tiles; the satellite
//! basemap is built in), other providers, files that are not there (checked by path only), file types the viewer
//! cannot read. Groups that end up empty are dropped. Tested without a window on a made-up project.

use super::gis_layers::Kind;
use std::path::{Path, PathBuf};

/// One layer of the project that the viewer can draw.
#[derive(Clone, Debug, PartialEq)]
pub struct QLayer {
    pub name: String,
    pub path: PathBuf,
    /// The table of a GeoPackage (`layername=`).
    pub table: Option<String>,
    /// A raster (image / GeoTIFF) rather than vector data.
    pub raster: bool,
    pub checked: bool,
    /// 0 to 1, as in the project.
    pub opacity: f32,
    /// The filter of the layer (the SQL condition QGIS keeps as `subset=` in the data source), `""` for none.
    pub filter: String,
    /// The attribute the layer is labelled with in the project, `""` for none.
    pub label: String,
    /// The layer's coordinate system as `epsg:NNNN` (the project's when the layer has none, as QGIS assumes), if known.
    pub crs: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Group { name: String, checked: bool, open: bool, children: Vec<Node> },
    Layer(QLayer),
}

/// The legend of a project and what had to be left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub nodes: Vec<Node>,
    /// `(layer name, why)`
    pub skipped: Vec<(String, String)>,
    pub empty_groups: usize,
    /// "QGIS" or "ArcGIS" (empty = QGIS), for the messages.
    pub source: &'static str,
    /// A remark for people about what the format could not give (shown after the summary).
    pub note: String,
}

/// What the viewer adds to its layer list for one node, flattened in list order (a group is followed by its children);
/// `parent` is the position of the group in this same list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub name: String,
    pub visible: bool,
    pub open: bool,
    pub parent: Option<usize>,
    pub layer: Option<QLayer>,
}

impl Project {
    pub fn layer_count(&self) -> usize {
        fn count(n: &[Node]) -> usize {
            n.iter().map(|x| match x {
                Node::Group { children, .. } => count(children),
                Node::Layer(_) => 1,
            }).sum()
        }
        count(&self.nodes)
    }

    /// The tree as rows in legend order (top of the legend first).
    pub fn rows(&self) -> Vec<Row> {
        fn walk(nodes: &[Node], parent: Option<usize>, out: &mut Vec<Row>) {
            for n in nodes {
                match n {
                    Node::Group { name, checked, open, children } => {
                        let at = out.len();
                        out.push(Row { kind: Kind::Group, name: name.clone(), visible: *checked, open: *open, parent, layer: None });
                        walk(children, Some(at), out);
                    }
                    Node::Layer(l) => out.push(Row { kind: if l.raster { Kind::Map } else { Kind::Vector }, name: l.name.clone(), visible: l.checked, open: true, parent, layer: Some(l.clone()) }),
                }
            }
        }
        let mut out = vec![];
        walk(&self.nodes, None, &mut out);
        out
    }

    /// One line for people: how many layers, what was left out.
    pub fn summary(&self) -> String {
        let mut s = format!("Added (n={}) layers from the {} project.", self.layer_count(), if self.source.is_empty() { "QGIS" } else { self.source });
        if !self.skipped.is_empty() {
            s += &format!(" Left out (n={}): ", self.skipped.len());
            let list: Vec<String> = self.skipped.iter().take(6).map(|(n, why)| format!("{n} ({why})")).collect();
            s += &list.join(", ");
            if self.skipped.len() > 6 {
                s += ", ...";
            }
            s.push('.');
        }
        if self.empty_groups > 0 {
            s += &format!(" Empty groups not added (n={}).", self.empty_groups);
        }
        if !self.note.is_empty() {
            s.push(' ');
            s += &self.note;
        }
        s
    }
}

/// Reads a `.qgz` / `.qgs` file.
pub fn read_project(path: &Path) -> Result<Project, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let text = if ext == "qgz" {
        use std::io::Read;
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| format!("Not a QGIS project archive: {e}"))?;
        let name = z.file_names().filter_map(|n| n.ok().map(|n| n.to_string())).find(|n| n.to_lowercase().ends_with(".qgs")).ok_or("The archive holds no .qgs project file.")?;
        let mut s = String::new();
        z.by_name(&name).map_err(|e| e.to_string())?.read_to_string(&mut s).map_err(|e| format!("Could not read the project: {e}"))?;
        s
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?
    };
    parse(&text, path.parent().unwrap_or(Path::new(".")), &|p| p.exists())
}

/// Parses the project XML. `base` is the folder relative paths start from; `exists` says whether a path is there (a
/// parameter so that the test needs no files).
pub fn parse(xml: &str, base: &Path, exists: &dyn Fn(&Path) -> bool) -> Result<Project, String> {
    let doc = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("The project file is damaged: {e}"))?;
    let root = doc.root_element();
    let tree = root.children().find(|n| n.has_tag_name("layer-tree-group")).ok_or("The project has no layer tree.")?;
    // opacity, labels and coordinate system by layer id, from the map layer section
    let authid = |n: roxmltree::Node| -> Option<String> {
        let a = n.descendants().find(|x| x.has_tag_name("authid")).and_then(|x| x.text())?;
        let code = a.trim().to_uppercase().strip_prefix("EPSG:")?.parse::<u32>().ok()?;
        Some(format!("epsg:{code}"))
    };
    let project_crs = root.children().find(|n| n.has_tag_name("projectCrs")).and_then(authid);
    let mut info = std::collections::HashMap::new();
    for ml in root.descendants().filter(|n| n.has_tag_name("maplayer")) {
        let Some(id) = ml.children().find(|n| n.has_tag_name("id")).and_then(|n| n.text()) else { continue };
        let opacity = ml
            .children()
            .find(|n| n.has_tag_name("layerOpacity"))
            .and_then(|n| n.text().and_then(|t| t.trim().parse::<f32>().ok()))
            .or_else(|| ml.descendants().find(|n| n.has_tag_name("rasterrenderer")).and_then(|n| n.attribute("opacity")).and_then(|t| t.parse::<f32>().ok()))
            .map(|o| o.clamp(0.0, 1.0));
        // labels: "Single labels" on a plain attribute (an expression is not carried over)
        let label = if ml.attribute("labelsEnabled") == Some("1") {
            ml.descendants().find(|n| n.has_tag_name("text-style") && n.attribute("isExpression") != Some("1")).and_then(|n| n.attribute("fieldName")).unwrap_or("").to_string()
        } else {
            String::new()
        };
        let crs = ml.children().find(|n| n.has_tag_name("srs")).and_then(authid);
        info.insert(id.to_string(), Info { opacity, label, crs });
    }
    let mut p = Project::default();
    let ctx = Ctx { base, exists, info: &info, project_crs };
    p.nodes = walk(tree, &ctx, &mut p.skipped, &mut p.empty_groups);
    Ok(p)
}

struct Info {
    opacity: Option<f32>,
    label: String,
    crs: Option<String>,
}

struct Ctx<'a> {
    base: &'a Path,
    exists: &'a dyn Fn(&Path) -> bool,
    info: &'a std::collections::HashMap<String, Info>,
    project_crs: Option<String>,
}

fn walk(group: roxmltree::Node, ctx: &Ctx, skipped: &mut Vec<(String, String)>, empty: &mut usize) -> Vec<Node> {
    let mut out = vec![];
    for n in group.children().filter(|n| n.is_element()) {
        let checked = n.attribute("checked").map_or(true, |c| c != "Qt::Unchecked");
        let name = n.attribute("name").unwrap_or("").to_string();
        if n.has_tag_name("layer-tree-group") {
            let children = walk(n, ctx, skipped, empty);
            if children.is_empty() {
                *empty += 1;
            } else {
                out.push(Node::Group { name, checked, open: n.attribute("expanded").map_or(true, |e| e == "1"), children });
            }
        } else if n.has_tag_name("layer-tree-layer") {
            let provider = n.attribute("providerKey").unwrap_or("");
            let source = n.attribute("source").unwrap_or("");
            let inf = n.attribute("id").and_then(|i| ctx.info.get(i));
            match layer(&name, provider, source, ctx.base, ctx.exists, checked, inf.and_then(|i| i.opacity).unwrap_or(1.0)) {
                Ok(mut l) => {
                    l.label = inf.map(|i| i.label.clone()).unwrap_or_default();
                    // a layer without a system of its own is in the project's (QGIS does the same)
                    l.crs = inf.and_then(|i| i.crs.clone()).or_else(|| ctx.project_crs.clone());
                    out.push(Node::Layer(l))
                }
                Err(why) => skipped.push((name, why)),
            }
        }
    }
    out
}

fn layer(name: &str, provider: &str, source: &str, base: &Path, exists: &dyn Fn(&Path) -> bool, checked: bool, opacity: f32) -> Result<QLayer, String> {
    let raster = match provider {
        "ogr" => false,
        "gdal" => true,
        "wms" | "wmts" | "xyz" | "arcgismapserver" | "vectortile" => return Err("online layer".into()),
        "" => return Err("no data source".into()),
        other => return Err(format!("{other} layer")),
    };
    let mut parts = source.split('|');
    let file = parts.next().unwrap_or("").trim();
    if file.is_empty() {
        return Err("no file".into());
    }
    let rest: Vec<&str> = parts.collect();
    let table = rest.iter().find_map(|p| p.strip_prefix("layername=")).map(|t| t.to_string());
    // the filter runs to the end of the source or to the next known key (a condition may contain a | itself)
    let filter = source
        .find("|subset=")
        .map(|i| {
            let t = &source[i + "|subset=".len()..];
            let end = ["|layername=", "|layerid=", "|geometrytype=", "|uniqueGeometryType=", "|option:"].iter().filter_map(|k| t.find(k)).min().unwrap_or(t.len());
            t[..end].trim().to_string()
        })
        .unwrap_or_default();
    let path = resolve(file, base);
    if !exists(&path) {
        return Err("file not found".into());
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let known = if raster { matches!(ext.as_str(), "tif" | "tiff" | "png" | "jpg" | "jpeg") } else { matches!(ext.as_str(), "shp" | "gpkg" | "geojson" | "json") };
    if !known {
        return Err(format!("{} files are not supported", if ext.is_empty() { "this type of".to_string() } else { format!(".{ext}") }));
    }
    Ok(QLayer { name: name.to_string(), path, table, raster, checked, opacity, filter, label: String::new(), crs: None })
}

/// A path of the project: absolute as it is, relative from the project's folder (`./` and `../` are folded away).
fn resolve(file: &str, base: &Path) -> PathBuf {
    let p = PathBuf::from(file);
    let joined = if p.is_absolute() { p } else { base.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'>
<qgis version="3.44.1">
 <layer-tree-group>
  <layer-tree-group name="Sites" checked="Qt::Checked" expanded="1">
   <layer-tree-layer id="a1" name="Collars" checked="Qt::Unchecked" providerKey="ogr" source="./data/collars.gpkg|layername=collars_2025|subset=&quot;HOLEID&quot; IN (&#xd;&#xa;&apos;A1&apos;, &apos;B2&apos;)"/>
   <layer-tree-group name="Selections" checked="Qt::Unchecked" expanded="0">
    <layer-tree-layer id="a2" name="Block samples" checked="Qt::Checked" providerKey="ogr" source="../shared/blocks.shp"/>
   </layer-tree-group>
  </layer-tree-group>
  <layer-tree-group name="Geophysics" checked="Qt::Checked" expanded="1">
   <layer-tree-layer id="r1" name="Magnetics" checked="Qt::Checked" providerKey="gdal" source="./grids/mag.tif"/>
   <layer-tree-layer id="r2" name="Missing grid" checked="Qt::Checked" providerKey="gdal" source="./grids/gone.tif"/>
  </layer-tree-group>
  <layer-tree-group name="satellite" checked="Qt::Checked" expanded="0">
   <layer-tree-layer id="w1" name="Google Satellite" checked="Qt::Checked" providerKey="wms" source="crs=EPSG:3857&amp;type=xyz&amp;url=https://example.org/{z}/{x}/{y}"/>
  </layer-tree-group>
  <layer-tree-group name="group3" checked="Qt::Checked" expanded="1"/>
  <layer-tree-layer id="c1" name="Table" checked="Qt::Checked" providerKey="delimitedtext" source="file:///x.csv"/>
 </layer-tree-group>
 <projectCrs><spatialrefsys><authid>EPSG:32643</authid></spatialrefsys></projectCrs>
 <projectlayers>
  <maplayer type="vector" labelsEnabled="1"><id>a1</id><layerOpacity>0.5</layerOpacity><srs><spatialrefsys><authid>EPSG:4326</authid></spatialrefsys></srs><labeling type="simple"><settings><text-style fieldName="HOLEID" isExpression="0"/></settings></labeling></maplayer>
  <maplayer type="raster"><id>r1</id><pipe><rasterrenderer opacity="0.8" type="singlebandgray"/></pipe></maplayer>
 </projectlayers>
</qgis>"#;

    fn here(p: &Path) -> bool {
        !p.to_string_lossy().contains("gone")
    }

    #[test]
    fn the_legend_keeps_its_groups_order_ticks_and_folding() {
        let p = parse(XML, Path::new("/proj/qgis"), &here).unwrap();
        let rows = p.rows();
        let names: Vec<(&str, Option<usize>, bool, bool)> = rows.iter().map(|r| (r.name.as_str(), r.parent, r.visible, r.open)).collect();
        assert_eq!(
            names,
            vec![
                ("Sites", None, true, true),
                ("Collars", Some(0), false, true),
                ("Selections", Some(0), false, false),
                ("Block samples", Some(2), true, true),
                ("Geophysics", None, true, true),
                ("Magnetics", Some(4), true, true),
            ]
        );
        assert_eq!(rows.iter().map(|r| r.kind).collect::<Vec<_>>(), vec![Kind::Group, Kind::Vector, Kind::Group, Kind::Vector, Kind::Group, Kind::Map]);
        assert_eq!(p.layer_count(), 3);
    }

    #[test]
    fn paths_are_taken_from_the_project_folder_and_the_table_from_layername() {
        let p = parse(XML, Path::new("/proj/qgis"), &here).unwrap();
        let l: Vec<QLayer> = p.rows().into_iter().filter_map(|r| r.layer).collect();
        assert_eq!(l[0].path, PathBuf::from("/proj/qgis/data/collars.gpkg"));
        assert_eq!(l[0].table.as_deref(), Some("collars_2025"));
        assert_eq!(l[1].path, PathBuf::from("/proj/shared/blocks.shp"));
        assert_eq!(l[1].table, None);
        assert_eq!((l[2].raster, l[2].path.clone()), (true, PathBuf::from("/proj/qgis/grids/mag.tif")));
        // opacity from the layer (vector) or the renderer (raster); 1 when the project says nothing
        assert_eq!((l[0].opacity, l[1].opacity, l[2].opacity), (0.5, 1.0, 0.8));
        // the filter (the whole condition, line breaks and all), the label attribute, the coordinate system of the layer or
        // else of the project
        assert_eq!(l[0].filter, "\"HOLEID\" IN (\r\n'A1', 'B2')");
        assert_eq!((l[1].filter.as_str(), l[0].label.as_str(), l[1].label.as_str()), ("", "HOLEID", ""));
        assert_eq!((l[0].crs.as_deref(), l[1].crs.as_deref(), l[2].crs.as_deref()), (Some("epsg:4326"), Some("epsg:32643"), Some("epsg:32643")));
    }

    #[test]
    fn what_cannot_be_shown_is_listed_with_the_reason_and_empty_groups_are_dropped() {
        let p = parse(XML, Path::new("/proj/qgis"), &here).unwrap();
        let why: Vec<(&str, &str)> = p.skipped.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert_eq!(why, vec![("Missing grid", "file not found"), ("Google Satellite", "online layer"), ("Table", "delimitedtext layer")]);
        // "satellite" (only the online layer) and "group3" (nothing) are not added
        assert_eq!(p.empty_groups, 2);
        assert!(p.rows().iter().all(|r| r.name != "satellite" && r.name != "group3"));
        assert!(p.summary().contains("(n=3) layers") && p.summary().contains("Google Satellite (online layer)"));
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused() {
        assert!(parse("<qgis></qgis>", Path::new("."), &here).unwrap_err().contains("no layer tree"));
        assert!(parse("not xml at all", Path::new("."), &here).unwrap_err().contains("damaged"));
    }

    /// `AMS_STUDIO_TEST_QGZ=<project> cargo test -p ams-studio a_real_project -- --ignored --nocapture`: reads a project of
    /// the user's (paths only) and prints the tree and what was left out. Not part of the normal run.
    #[test]
    #[ignore]
    fn a_real_project_can_be_read() {
        let Some(p) = std::env::var_os("AMS_STUDIO_TEST_QGZ") else { return };
        let proj = read_project(Path::new(&p)).unwrap();
        for r in proj.rows() {
            let depth = std::iter::successors(r.parent, |&k| proj.rows()[k].parent).count();
            println!("{}{} {:?} {}{}", "  ".repeat(depth), r.name, r.kind, if r.visible { "on" } else { "off" }, r.layer.as_ref().map_or(String::new(), |l| format!("  [{}{}]", l.path.file_name().unwrap().to_string_lossy(), l.table.as_ref().map_or(String::new(), |t| format!(" | {t}")))));
        }
        println!("{}", proj.summary());
        // every filter of the project has to be read by the filter engine (the condition only, no data)
        let (mut with, mut bad) = (0, 0);
        for l in proj.rows().into_iter().filter_map(|r| r.layer).filter(|l| !l.filter.is_empty()) {
            with += 1;
            if let Err(e) = super::super::gis_filter::parse(&l.filter) {
                bad += 1;
                println!("filter of {} not read: {e}", l.name);
            }
        }
        println!("layers with a filter: {with}, not read: {bad}; with labels: {}; with a system: {}", proj.rows().iter().filter_map(|r| r.layer.as_ref()).filter(|l| !l.label.is_empty()).count(), proj.rows().iter().filter_map(|r| r.layer.as_ref()).filter(|l| l.crs.is_some()).count());
    }
}
