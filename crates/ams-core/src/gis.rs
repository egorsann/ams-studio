//! Map maths for the GIS viewer: projections (lon/lat, Web Mercator, UTM), the fit of a map image to the earth from
//! control points, world files, angles typed in a table, web map tiles and the strike / dip and lineation symbols of
//! an AMS fabric. No GUI and no file access, so everything here is testable.
//!
//! Everything on screen lives in Web Mercator metres (EPSG:3857, north up), the system the OpenStreetMap tiles use.
//! A map image carries its own system (`Crs`) and an affine transform from pixel to that system; images that are
//! georeferenced with control points are fitted straight to Web Mercator.

use std::f64::consts::PI;

const WGS_A: f64 = 6_378_137.0;
const WGS_F: f64 = 1.0 / 298.257_223_563;
/// Largest latitude the Web Mercator square holds.
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// Web Mercator metres from longitude / latitude in degrees.
pub fn merc_from_lonlat(lon: f64, lat: f64) -> (f64, f64) {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT);
    (WGS_A * lon.to_radians(), WGS_A * (PI / 4.0 + lat.to_radians() / 2.0).tan().ln())
}

/// Longitude / latitude in degrees from Web Mercator metres.
pub fn lonlat_from_merc(x: f64, y: f64) -> (f64, f64) {
    (x / WGS_A * 180.0 / PI, (2.0 * (y / WGS_A).exp().atan() - PI / 2.0) * 180.0 / PI)
}

/// Metres on the ground per Web Mercator metre at a latitude (Web Mercator stretches by 1 / cos(lat)).
pub fn merc_scale(lat: f64) -> f64 {
    lat.to_radians().cos().max(0.01)
}

fn tm_consts() -> (f64, [f64; 4], [f64; 4], [f64; 4]) {
    let n = WGS_F / (2.0 - WGS_F);
    let (n2, n3, n4) = (n * n, n * n * n, n * n * n * n);
    let a = WGS_A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0);
    let alpha = [
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0,
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0,
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0,
        49561.0 * n4 / 161280.0,
    ];
    let beta = [
        n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0,
        n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0,
        17.0 * n3 / 480.0 - 37.0 * n4 / 840.0,
        4397.0 * n4 / 161280.0,
    ];
    let delta = [
        2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0,
        7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0,
        56.0 * n3 / 15.0 - 136.0 * n4 / 35.0,
        4279.0 * n4 / 630.0,
    ];
    (a, alpha, beta, delta)
}

const K0: f64 = 0.9996;

/// UTM (WGS 84) easting / northing in metres from longitude / latitude, for a zone (1-60) and hemisphere.
pub fn utm_from_lonlat(lon: f64, lat: f64, zone: u8, north: bool) -> (f64, f64) {
    let (a, alpha, _, _) = tm_consts();
    let n = WGS_F / (2.0 - WGS_F);
    let lon0 = (f64::from(zone) * 6.0 - 183.0).to_radians();
    let (phi, dl) = (lat.to_radians(), lon.to_radians() - lon0);
    let c = 2.0 * n.sqrt() / (1.0 + n);
    let t = (phi.sin().atanh() - c * (c * phi.sin()).atanh()).sinh();
    let xi0 = t.atan2(dl.cos());
    let eta0 = (dl.sin() / (1.0 + t * t).sqrt()).atanh();
    let (mut xi, mut eta) = (xi0, eta0);
    for (j, al) in alpha.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi += al * (k * xi0).sin() * (k * eta0).cosh();
        eta += al * (k * xi0).cos() * (k * eta0).sinh();
    }
    (500_000.0 + K0 * a * eta, if north { 0.0 } else { 10_000_000.0 } + K0 * a * xi)
}

/// Longitude / latitude in degrees from UTM (WGS 84) easting / northing.
pub fn lonlat_from_utm(e: f64, n_: f64, zone: u8, north: bool) -> (f64, f64) {
    let (a, _, beta, delta) = tm_consts();
    let lon0 = f64::from(zone) * 6.0 - 183.0;
    let xi = (n_ - if north { 0.0 } else { 10_000_000.0 }) / (K0 * a);
    let eta = (e - 500_000.0) / (K0 * a);
    let (mut xi1, mut eta1) = (xi, eta);
    for (j, b) in beta.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi1 -= b * (k * xi).sin() * (k * eta).cosh();
        eta1 -= b * (k * xi).cos() * (k * eta).sinh();
    }
    let chi = (xi1.sin() / eta1.cosh()).asin();
    let mut phi = chi;
    for (j, d) in delta.iter().enumerate() {
        phi += d * (2.0 * (j as f64 + 1.0) * chi).sin();
    }
    (lon0 + eta1.sinh().atan2(xi1.cos()).to_degrees(), phi.to_degrees())
}

/// The UTM zone of a longitude.
pub fn utm_zone(lon: f64) -> u8 {
    (((lon + 180.0) / 6.0).floor() as i32 + 1).clamp(1, 60) as u8
}

/// A coordinate system a map or a table can be in.
///
/// The first three are computed here (fast, and exact for the usual cases); every other system of the EPSG registry
/// (about 6000 geographic and projected systems: Gauss-Krueger, State Plane, Lambert, Albers, polar stereographic,
/// national grids, ...) goes through the pure Rust PROJ port `proj4rs`, with the datum shift to WGS 84 that the
/// registry gives (three or seven parameters; grid shifts such as OSTN15 are not applied, so the British National
/// Grid is good to a few metres). `Custom` is a PROJ string typed by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Crs {
    /// Longitude (x) / latitude (y) in degrees, WGS 84 or anything that agrees with it to a few metres.
    LonLat,
    /// Web Mercator metres (EPSG:3857).
    WebMerc,
    /// UTM on WGS 84, ETRS89 or NAD83 (they differ by a metre or two, far below the size of a map symbol).
    Utm { zone: u8, north: bool },
    /// Any other EPSG code (4 to 5 digits) that the registry holds.
    Epsg(u16),
    /// A PROJ string given by the user (index into a list kept for the session).
    Custom(u16),
}

static CUSTOM: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

thread_local! {
    static PROJ_CACHE: std::cell::RefCell<std::collections::HashMap<Crs, Option<std::rc::Rc<proj4rs::Proj>>>> = std::cell::RefCell::new(std::collections::HashMap::new());
    static WGS84: std::rc::Rc<proj4rs::Proj> = std::rc::Rc::new(proj4rs::Proj::from_proj_string("+proj=longlat +datum=WGS84 +no_defs").expect("WGS 84"));
}

fn proj_of(c: &Crs) -> Option<std::rc::Rc<proj4rs::Proj>> {
    PROJ_CACHE.with(|cache| {
        if let Some(p) = cache.borrow().get(c) {
            return p.clone();
        }
        let made = match *c {
            Crs::Epsg(code) => proj4rs::Proj::from_epsg_code(code).ok(),
            Crs::Custom(i) => CUSTOM.lock().ok().and_then(|v| v.get(i as usize).cloned()).and_then(|d| proj4rs::Proj::from_proj_string(&d).ok()),
            _ => None,
        }
        .map(std::rc::Rc::new);
        cache.borrow_mut().insert(*c, made.clone());
        made
    })
}

/// Transforms one point between two systems of proj4rs; angles in degrees on both sides, NaN when it fails.
fn transform_pt(from: &proj4rs::Proj, to: &proj4rs::Proj, x: f64, y: f64) -> (f64, f64) {
    let mut p = (x, y, 0.0);
    if from.is_latlong() {
        p.0 = p.0.to_radians();
        p.1 = p.1.to_radians();
    }
    if proj4rs::transform::transform(from, to, &mut p).is_err() {
        return (f64::NAN, f64::NAN);
    }
    if to.is_latlong() {
        (p.0.to_degrees(), p.1.to_degrees())
    } else {
        (p.0, p.1)
    }
}

/// The name of an EPSG system from the registry (`Pulkovo 1942 / Gauss-Kruger zone 12`).
pub fn epsg_name(code: u16) -> Option<String> {
    let wkt = crs_definitions::from_code(code)?.wkt;
    let start = wkt.find('"')? + 1;
    let end = start + wkt[start..].find('"')?;
    Some(wkt[start..end].to_string())
}

/// Every geographic and projected system of the registry as (code, name), built once.
fn epsg_list() -> &'static Vec<(u16, String)> {
    static LIST: std::sync::OnceLock<Vec<(u16, String)>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        (1..=u16::MAX)
            .filter_map(|c| {
                let d = crs_definitions::from_code(c)?;
                if !(d.wkt.starts_with("PROJCS[") || d.wkt.starts_with("GEOGCS[")) {
                    return None;
                }
                Some((c, epsg_name(c)?))
            })
            .collect()
    })
}

/// Systems whose code or name contains every word of `query` (capitals do not matter), best first, at most `limit`.
pub fn epsg_search(query: &str, limit: usize) -> Vec<(u16, String)> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return vec![];
    }
    let mut hits: Vec<(u8, usize, u16, &String)> = epsg_list()
        .iter()
        .filter_map(|(c, n)| {
            let low = n.to_lowercase();
            let code = c.to_string();
            if !words.iter().all(|w| low.contains(w.as_str()) || code.starts_with(w.as_str())) {
                return None;
            }
            let rank = if words.len() == 1 && code == words[0] { 0 } else if words.iter().any(|w| code == *w) { 1 } else { 2 };
            Some((rank, n.len(), *c, n))
        })
        .collect();
    hits.sort();
    hits.into_iter().take(limit).map(|h| (h.2, h.3.clone())).collect()
}

fn norm_name(s: &str) -> String {
    let mut t: String = s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    for (from, to) in [("wgs1984", "wgs84"), ("nad1983", "nad83"), ("etrs1989", "etrs89"), ("gk", "gausskruger"), ("gcs", ""), ("d", "")] {
        if from == "d" {
            // the ESRI datum prefix D_ only at the start of a name
            if t.starts_with("d") && s.to_lowercase().starts_with("d_") {
                t.remove(0);
            }
        } else if from == "gcs" {
            if t.starts_with("gcs") {
                t = t[3..].to_string();
            }
        } else {
            t = t.replace(from, to);
        }
    }
    t
}

/// The EPSG code whose name matches the (ESRI style) name of a `.prj`: `Pulkovo_1942_GK_Zone_13`, `British_National_Grid`.
fn epsg_by_name(name: &str) -> Option<u16> {
    let n = norm_name(name);
    if n.len() < 4 {
        return None;
    }
    let list = epsg_list();
    // projected systems before geographic ones; an exact name before a name that ends with it
    let pick = |f: &dyn Fn(&str) -> bool| list.iter().filter(|(_, nm)| f(&norm_name(nm))).min_by_key(|(c, nm)| (nm.len(), *c)).map(|(c, _)| *c);
    pick(&|x| x == n).or_else(|| pick(&|x| x.ends_with(&n)))
}

impl Crs {
    /// What an EPSG code means here, `None` for the codes that the registry does not hold or `proj4rs` cannot build.
    pub fn from_epsg(code: u32) -> Option<Crs> {
        match code {
            4326 | 4269 | 4258 | 4979 | 4617 | 4937 => Some(Crs::LonLat),
            3857 | 900_913 | 3785 | 102_100 | 102_113 => Some(Crs::WebMerc),
            32_601..=32_660 => Some(Crs::Utm { zone: (code - 32_600) as u8, north: true }),
            32_701..=32_760 => Some(Crs::Utm { zone: (code - 32_700) as u8, north: false }),
            25_828..=25_838 => Some(Crs::Utm { zone: (code - 25_800) as u8, north: true }), // ETRS89 UTM 28-38
            26_901..=26_923 => Some(Crs::Utm { zone: (code - 26_900) as u8, north: true }), // NAD83 UTM 1-23
            c if c <= u32::from(u16::MAX) => {
                let c = Crs::Epsg(c as u16);
                proj_of(&c).map(|_| c)
            }
            _ => None,
        }
    }

    /// A system from text typed by the user: `28412`, `EPSG:28412`, or a PROJ string (`+proj=tmerc +lat_0=0 ...`).
    pub fn from_definition(text: &str) -> Result<Crs, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("Type an EPSG code (for example 28412) or a PROJ string.".into());
        }
        if t.starts_with('+') {
            let v = proj4rs::Proj::from_proj_string(t).map_err(|e| format!("The PROJ string is not usable: {e}"))?;
            if v.has_inverse() && v.has_forward() {
                let mut list = CUSTOM.lock().map_err(|_| "internal error".to_string())?;
                let i = match list.iter().position(|d| d == t) {
                    Some(i) => i,
                    None => {
                        list.push(t.to_string());
                        list.len() - 1
                    }
                };
                return Ok(Crs::Custom(i as u16));
            }
            return Err("That projection cannot be inverted.".into());
        }
        let digits = t.trim_start_matches(|c: char| !c.is_ascii_digit());
        let code: u32 = digits.parse().map_err(|_| "Type an EPSG code (for example 28412) or a PROJ string.".to_string())?;
        Crs::from_epsg(code).ok_or_else(|| format!("EPSG:{code} is not in the list of systems AMS Studio knows (search the list, or type a PROJ string)."))
    }

    /// The PROJ string of a custom system (to save it with a georeference).
    pub fn custom_definition(&self) -> Option<String> {
        match *self {
            Crs::Custom(i) => CUSTOM.lock().ok().and_then(|v| v.get(i as usize).cloned()),
            _ => None,
        }
    }

    /// Short text for the interface.
    pub fn label(&self) -> String {
        match self {
            Crs::LonLat => "longitude / latitude (WGS 84)".into(),
            Crs::WebMerc => "Web Mercator (EPSG:3857)".into(),
            Crs::Utm { zone, north } => format!("UTM zone {zone}{}", if *north { "N" } else { "S" }),
            Crs::Epsg(c) => match epsg_name(*c) {
                Some(n) => format!("{n} (EPSG:{c})"),
                None => format!("EPSG:{c}"),
            },
            Crs::Custom(_) => "custom PROJ definition".into(),
        }
    }

    /// A short name for lists: `UTM 44N`, `EPSG:28414`.
    pub fn short_label(&self) -> String {
        match self {
            Crs::LonLat => "WGS 84".into(),
            Crs::WebMerc => "Web Mercator".into(),
            Crs::Utm { zone, north } => format!("UTM {zone}{}", if *north { "N" } else { "S" }),
            Crs::Epsg(c) => format!("EPSG:{c}"),
            Crs::Custom(_) => "PROJ string".into(),
        }
    }

    /// Longitude / latitude (WGS 84) of a point given in this system; NaN when it cannot be converted.
    pub fn to_lonlat(&self, x: f64, y: f64) -> (f64, f64) {
        match *self {
            Crs::LonLat => (x, y),
            Crs::WebMerc => lonlat_from_merc(x, y),
            Crs::Utm { zone, north } => lonlat_from_utm(x, y, zone, north),
            _ => match proj_of(self) {
                Some(p) => WGS84.with(|w| transform_pt(&p, w, x, y)),
                None => (f64::NAN, f64::NAN),
            },
        }
    }

    /// This system's coordinates of a longitude / latitude (WGS 84).
    pub fn from_lonlat(&self, lon: f64, lat: f64) -> (f64, f64) {
        match *self {
            Crs::LonLat => (lon, lat),
            Crs::WebMerc => merc_from_lonlat(lon, lat),
            Crs::Utm { zone, north } => utm_from_lonlat(lon, lat, zone, north),
            _ => match proj_of(self) {
                Some(p) => WGS84.with(|w| transform_pt(w, &p, lon, lat)),
                None => (f64::NAN, f64::NAN),
            },
        }
    }

    /// Web Mercator metres of a point given in this system.
    pub fn to_merc(&self, x: f64, y: f64) -> (f64, f64) {
        match self {
            Crs::WebMerc => (x, y),
            _ => {
                let (lo, la) = self.to_lonlat(x, y);
                merc_from_lonlat(lo, la)
            }
        }
    }

    /// Reads the system out of the text of a `.prj` file (WKT, ESRI or OGC): the last EPSG authority code, a UTM zone
    /// name, or the name matched against the registry (`Pulkovo_1942_GK_Zone_13`). `None` when it cannot be told.
    pub fn from_prj(text: &str) -> Option<Crs> {
        if let Some(code) = text.rfind("AUTHORITY[\"EPSG\",\"").or_else(|| text.rfind("ID[\"EPSG\",")).and_then(|p| {
            let rest = &text[p..];
            let digits: String = rest.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok()
        }) {
            if let Some(c) = Crs::from_epsg(code) {
                return Some(c);
            }
        }
        let low = text.to_lowercase();
        let wgs_like = ["wgs_1984", "wgs 84", "wgs84", "etrs89", "etrs_1989", "nad83", "nad_1983"].iter().any(|k| low.contains(k));
        if wgs_like {
            if let Some(p) = low.find("utm_zone_").or_else(|| low.find("utm zone ")) {
                let rest = &low[p + 9..];
                let zone: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let hemi = rest.chars().nth(zone.len());
                if let Ok(z) = zone.parse::<u8>() {
                    if (1..=60).contains(&z) {
                        return Some(Crs::Utm { zone: z, north: hemi != Some('s') });
                    }
                }
            }
        }
        if low.contains("web_mercator") || low.contains("pseudo-mercator") || low.contains("pseudo_mercator") {
            return Some(Crs::WebMerc);
        }
        // the first quoted name of the WKT: PROJCS["Pulkovo_1942_GK_Zone_13", ...] or GEOGCS["GCS_Pulkovo_1942", ...]
        let name = text.split('"').nth(1)?;
        if (low.starts_with("geogcs") || low.starts_with("geodcrs")) && wgs_like && !low.contains("projcs") {
            return Some(Crs::LonLat);
        }
        epsg_by_name(name).and_then(|c| Crs::from_epsg(u32::from(c)))
    }
}

/// X = a px + b py + c, Y = d px + e py + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub fn apply(&self, px: f64, py: f64) -> (f64, f64) {
        (self.a * px + self.b * py + self.c, self.d * px + self.e * py + self.f)
    }

    /// The transform the other way round (`None` for a flat one).
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.e - self.b * self.d;
        if det.abs() < 1e-300 || !det.is_finite() {
            return None;
        }
        let (a, b, d, e) = (self.e / det, -self.b / det, -self.d / det, self.a / det);
        Some(Affine { a, b, c: -(a * self.c + b * self.f), d, e, f: -(d * self.c + e * self.f) })
    }
}

/// A pair of control points: the pixel on the map image, and the Web Mercator metres it stands for.
pub type ControlPoint = ((f64, f64), (f64, f64));

/// Fits pixel -> Web Mercator from control points. Two points give a similarity (scale, rotation, shift; pixels
/// stay square, image y down and north up), three or more a least-squares affine transform.
pub fn fit_affine(pts: &[ControlPoint]) -> Result<Affine, String> {
    let n = pts.len();
    if n < 2 {
        return Err("At least two control points are needed (three or more are better).".into());
    }
    // centre both sides to keep the numbers well conditioned
    let (mut spx, mut spy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    for ((px, py), (x, y)) in pts {
        spx += px;
        spy += py;
        sx += x;
        sy += y;
    }
    let nf = n as f64;
    let (cpx, cpy, cx, cy) = (spx / nf, spy / nf, sx / nf, sy / nf);
    if n == 2 {
        let ((p0x, p0y), (x0, y0)) = pts[0];
        let ((p1x, p1y), (x1, y1)) = pts[1];
        let (dpx, dpy, dx, dy) = (p1x - p0x, p1y - p0y, x1 - x0, y1 - y0);
        let den = dpx * dpx + dpy * dpy;
        if den < 1e-12 {
            return Err("The two points on the map image are the same point.".into());
        }
        // image y runs down and north up, so the map is a rotated, mirrored copy of the image:
        // X + iY = (u + iv)(px - i py), which is X = u px + v py + c, Y = v px - u py + f
        // divide dx + i dy by dpx - i dpy
        let u = (dx * dpx - dy * dpy) / den;
        let v = (dx * dpy + dy * dpx) / den;
        if u * u + v * v < 1e-30 {
            return Err("The two points on the base map are the same point.".into());
        }
        return Ok(Affine { a: u, b: v, c: x0 - (u * p0x + v * p0y), d: v, e: -u, f: y0 - (v * p0x - u * p0y) });
    }
    // normal equations for [a b c] and [d e f]
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    let (mut bx1, mut bx2, mut by1, mut by2) = (0.0, 0.0, 0.0, 0.0);
    for ((px, py), (x, y)) in pts {
        let (u, v) = (px - cpx, py - cpy);
        sxx += u * u;
        sxy += u * v;
        syy += v * v;
        bx1 += u * (x - cx);
        bx2 += v * (x - cx);
        by1 += u * (y - cy);
        by2 += v * (y - cy);
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 * (sxx * syy).abs().max(1e-300) {
        return Err("The control points lie on a line on the map image; spread them over the map (three or more, not in a row).".into());
    }
    let a = (bx1 * syy - bx2 * sxy) / det;
    let b = (bx2 * sxx - bx1 * sxy) / det;
    let d = (by1 * syy - by2 * sxy) / det;
    let e = (by2 * sxx - by1 * sxy) / det;
    Ok(Affine { a, b, c: cx - a * cpx - b * cpy, d, e, f: cy - d * cpx - e * cpy })
}

/// Distance in metres (Web Mercator metres, corrected for latitude) between each control point and where the fit
/// puts it, and the root mean square of them.
pub fn residuals(pts: &[ControlPoint], t: &Affine) -> (Vec<f64>, f64) {
    let r: Vec<f64> = pts
        .iter()
        .map(|((px, py), (x, y))| {
            let (fx, fy) = t.apply(*px, *py);
            let lat = lonlat_from_merc(*x, *y).1;
            ((fx - x).powi(2) + (fy - y).powi(2)).sqrt() * merc_scale(lat)
        })
        .collect();
    let rms = if r.is_empty() { 0.0 } else { (r.iter().map(|v| v * v).sum::<f64>() / r.len() as f64).sqrt() };
    (r, rms)
}

/// A map image on the earth: pixel -> coordinates of `crs` by an affine transform.
#[derive(Clone, Copy, Debug)]
pub struct Georef {
    pub affine: Affine,
    pub crs: Crs,
}

impl Georef {
    /// Web Mercator metres of a pixel (pixel centres are at +0.5; the transform here uses the pixel corner grid, so
    /// callers pass fractional pixel positions of the image).
    pub fn pixel_to_merc(&self, px: f64, py: f64) -> (f64, f64) {
        let (x, y) = self.affine.apply(px, py);
        self.crs.to_merc(x, y)
    }

    /// The pixel of a Web Mercator point (exact for Web Mercator maps, through longitude / latitude for others).
    pub fn merc_to_pixel(&self, mx: f64, my: f64) -> Option<(f64, f64)> {
        let (x, y) = match self.crs {
            Crs::WebMerc => (mx, my),
            c => {
                let (lo, la) = lonlat_from_merc(mx, my);
                c.from_lonlat(lo, la)
            }
        };
        self.affine.inverse().map(|i| i.apply(x, y))
    }
}

/// A world file (`.pgw`, `.jgw`, `.tfw`, `.wld`, six numbers): pixel *centre* of the top-left pixel is (C, F). Gives
/// the transform for pixel corner coordinates.
pub fn parse_world_file(text: &str) -> Option<Affine> {
    let v: Vec<f64> = text.lines().filter_map(|l| l.trim().replace(',', ".").parse::<f64>().ok()).collect();
    if v.len() < 6 {
        return None;
    }
    // A, D, B, E, C, F in file order
    let (a, d, b, e, c, f) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    let t = Affine { a, b, c: c - 0.5 * a - 0.5 * b, d, e, f: f - 0.5 * d - 0.5 * e };
    t.inverse().map(|_| t)
}

/// An angle or coordinate typed in a table: decimal degrees or metres with a point or a comma, `56.34`, `56,34`,
/// `-2.8`, `56 20.4 N`, `56°20'24.5"N`, `2.8W`. A trailing S or W makes it negative. Returns the number.
pub fn parse_angle(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.replace(',', ".").parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let up = t.to_uppercase();
    let neg_letter = up.contains('S') || up.contains('W');
    let letters = up.chars().any(|c| matches!(c, 'N' | 'S' | 'E' | 'W'));
    let cleaned: String = t
        .chars()
        .map(|c| if c.is_ascii_digit() || c == '.' || c == '-' { c } else if c == ',' { '.' } else { ' ' })
        .collect();
    let parts: Vec<f64> = cleaned.split_whitespace().filter_map(|p| p.parse::<f64>().ok()).collect();
    if parts.is_empty() || parts.len() > 3 || !(letters || t.contains('°') || parts.len() > 1) {
        return None;
    }
    let neg_sign = parts[0] < 0.0 || cleaned.trim_start().starts_with('-');
    let mut v = parts[0].abs();
    if let Some(m) = parts.get(1) {
        v += m.abs() / 60.0;
    }
    if let Some(sec) = parts.get(2) {
        v += sec.abs() / 3600.0;
    }
    Some(if neg_sign || neg_letter { -v } else { v })
}

/// A csv as a grid of cells: comma, semicolon or tab separated (whichever the first line uses most), quotes may hold
/// the separator (`"56,34"`), a leading byte order mark is dropped, empty trailing lines are dropped.
pub fn grid_from_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let outside_quotes = |ch: char| {
        let mut q = false;
        let mut n = 0;
        for c in first.chars() {
            if c == '"' {
                q = !q;
            } else if c == ch && !q {
                n += 1;
            }
        }
        n
    };
    let (nc, ns, nt) = (outside_quotes(','), outside_quotes(';'), outside_quotes('\t'));
    if nc == 0 && ns == 0 && nt == 0 && first.split_whitespace().count() >= 2 {
        // columns separated by runs of spaces
        let mut rows: Vec<Vec<String>> = text.lines().map(|l| l.split_whitespace().map(str::to_string).collect()).collect();
        while rows.last().is_some_and(|r| r.is_empty()) {
            rows.pop();
        }
        return rows;
    }
    let sep = if ns > nc && ns >= nt { ';' } else if nt > nc && nt > ns { '\t' } else { ',' };
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut cell, mut quoted) = (Vec::<String>::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => row.push(std::mem::take(&mut cell).trim().to_string()),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell).trim().to_string());
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell.trim().to_string());
        rows.push(row);
    }
    while rows.last().is_some_and(|r| r.iter().all(|c| c.is_empty())) {
        rows.pop();
    }
    rows
}

/// How a specimen name is matched to the name in the coordinates table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    /// The same text (capitals do not matter).
    Exact,
    /// The specimen name begins with the table name: one table row for a block, `B12` covers `B12-1`, `B12b`.
    StartsWithKey,
    /// The table has no usable names: row n belongs to the n-th specimen of the file.
    RowOrder,
}

fn class(c: char) -> u8 {
    if c.is_ascii_digit() {
        1
    } else if c.is_alphabetic() {
        2
    } else {
        0
    }
}

/// For each specimen name the index of the table key that belongs to it. With `StartsWithKey` the longest key that
/// begins the name wins, and the name must go on with a different kind of character (`B1` covers `B1-2` and `B1a`
/// but not `B10-2`).
pub fn match_names(specimens: &[String], keys: &[String], mode: MatchMode) -> Vec<Option<usize>> {
    if mode == MatchMode::RowOrder {
        return (0..specimens.len()).map(|i| (i < keys.len()).then_some(i)).collect();
    }
    let low: Vec<String> = keys.iter().map(|k| k.trim().to_lowercase()).collect();
    specimens
        .iter()
        .map(|sp| {
            let s = sp.trim().to_lowercase();
            if s.is_empty() {
                return None;
            }
            if let Some(i) = low.iter().position(|k| *k == s) {
                return Some(i);
            }
            if mode == MatchMode::Exact {
                return None;
            }
            let mut best: Option<(usize, usize)> = None;
            for (i, k) in low.iter().enumerate() {
                if k.is_empty() || !s.starts_with(k.as_str()) {
                    continue;
                }
                let last = class(k.chars().last().unwrap_or(' '));
                let next = s[k.len()..].chars().next().map_or(0, class);
                if last != 0 && last == next {
                    continue;
                }
                if best.map_or(true, |(_, l)| k.len() > l) {
                    best = Some((i, k.len()));
                }
            }
            best.map(|(i, _)| i)
        })
        .collect()
}

/// What the numbers a user typed look like.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub crs: Crs,
    /// x (longitude or easting) and y (latitude or northing) in `crs`, in the order the system wants them.
    pub x: f64,
    pub y: f64,
    /// A sentence for the user: what the numbers look like and what had to be assumed.
    pub what: String,
    /// The first typed number is the latitude / northing (the order was swapped to give x and y).
    pub swapped: bool,
}

fn hemisphere(s: &str) -> Option<char> {
    s.chars().map(|c| c.to_ascii_uppercase()).find(|c| matches!(c, 'N' | 'S' | 'E' | 'W'))
}

/// Guesses the coordinate system from two typed numbers (`47.1234`, `10°32'35"E`, `412345.678`).
///
/// * both small: degrees on WGS 84; the hemisphere letters or a value above 90 tell the order, else the written
///   convention latitude, longitude is assumed;
/// * 6-digit easting and a northing below 10 million: UTM, whose zone and hemisphere are not in the numbers
///   (`zone_hint` and `north_hint`, or the zone of `last`, the system confirmed for the previous specimen; the user
///   edits them next to the question);
/// * 7 or 8 digit easting with the zone number in front (12 412345): Gauss-Krueger on Pulkovo 1942;
/// * anything else: the previous system if there is one, otherwise an error (the user picks the system by name).
pub fn detect_crs(a: &str, b: &str, zone_hint: u8, north_hint: bool, last: Option<Crs>) -> Result<Detection, String> {
    let (Some(va), Some(vb)) = (parse_angle(a), parse_angle(b)) else {
        return Err("Type two numbers: the two coordinates.".into());
    };
    let (ha, hb) = (hemisphere(a), hemisphere(b));
    let degrees_like = va.abs() <= 180.0 && vb.abs() <= 180.0 && (va.abs() <= 90.0 || vb.abs() <= 90.0);
    if degrees_like {
        let lat_first = match (ha, hb) {
            (Some('N' | 'S'), _) | (_, Some('E' | 'W')) => Some(true),
            (Some('E' | 'W'), _) | (_, Some('N' | 'S')) => Some(false),
            _ => None,
        };
        let assumed = lat_first.is_none() && va.abs() <= 90.0 && vb.abs() <= 90.0;
        let lat_first = lat_first.unwrap_or_else(|| if va.abs() > 90.0 { false } else if vb.abs() > 90.0 { true } else { true });
        let (lat, lon) = if lat_first { (va, vb) } else { (vb, va) };
        if lat.abs() > 90.0 {
            return Err("A latitude is between -90 and 90 degrees.".into());
        }
        let mut what = "degrees (longitude / latitude on WGS 84)".to_string();
        if assumed {
            what += &format!(", assumed latitude {lat} first and longitude {lon} second (the written order; the numbers would also fit the other way round)");
        }
        return Ok(Detection { crs: Crs::LonLat, x: lon, y: lat, what, swapped: lat_first });
    }
    // the system settled for the previous specimen (other than UTM, which has its own branch) wins when it fits
    if let Some(c @ (Crs::Epsg(_) | Crs::Custom(_) | Crs::WebMerc)) = last {
        let (lo, la) = c.to_lonlat(va, vb);
        if lo.is_finite() && la.is_finite() && la.abs() < MAX_LAT && lo.abs() <= 180.0 {
            return Ok(Detection { crs: c, x: va, y: vb, what: format!("the same system as the previous specimen ({})", c.label()), swapped: false });
        }
    }
    let utm_like = |e: f64, n: f64| (100_000.0..900_000.0).contains(&e) && (0.0..=10_000_000.0).contains(&n);
    let (e, n, swapped) = if utm_like(va, vb) { (va, vb, false) } else if utm_like(vb, va) { (vb, va, true) } else { (va, vb, false) };
    if utm_like(e, n) {
        let (zone, north) = match last {
            Some(Crs::Utm { zone, north }) => (zone, north),
            _ => (zone_hint.clamp(1, 60), north_hint),
        };
        let mut what = format!("UTM metres (easting {e}, northing {n}); the zone and the hemisphere are not in the numbers, so zone {zone}{} is only the starting point: set them below", if north { "N" } else { "S" });
        if swapped {
            what += "; the first number looks like the northing, so the order was swapped";
        }
        return Ok(Detection { crs: Crs::Utm { zone, north }, x: e, y: n, what, swapped });
    }
    // Gauss-Krueger with the zone number in front of the easting (12 412345 = zone 12)
    if (1_000_000.0..33_000_000.0).contains(&va) && (0.0..=10_000_000.0).contains(&vb) {
        let zone = (va / 1_000_000.0).floor() as u32;
        let rest = va - f64::from(zone) * 1_000_000.0;
        if (4..=32).contains(&zone) && (100_000.0..900_000.0).contains(&rest) {
            if let Some(crs) = Crs::from_epsg(28_400 + zone) {
                return Ok(Detection { crs, x: va, y: vb, what: format!("Gauss-Krueger zone {zone} on Pulkovo 1942 (the zone number {zone} stands in front of the easting {rest})"), swapped: false });
            }
        }
    }
    if let Some(c) = last {
        let (lo, la) = c.to_lonlat(va, vb);
        if lo.is_finite() && la.is_finite() && la.abs() < MAX_LAT {
            return Ok(Detection { crs: c, x: va, y: vb, what: format!("the same system as the previous specimen ({})", c.label()), swapped: false });
        }
    }
    Err("The numbers do not look like degrees, UTM or Gauss-Krueger metres: choose the coordinate system by name.".into())
}

/// How well a table name `key` fits the specimen id: 6 same name (capitals, spaces and dashes ignored), 5 one holds
/// the other, 4 same letters and the same number (zero padding ignored), 2 same letters with another number, 0 nothing.
pub fn name_score(id: &str, key: &str) -> u8 {
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let (a, b) = (norm(id), norm(key));
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    if a == b {
        return 6;
    }
    if a.len().min(b.len()) >= 3 && (a.contains(&b) || b.contains(&a)) {
        return 5;
    }
    let split = |s: &str| {
        let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
        (s[..s.len() - digits.len()].to_string(), digits.parse::<u64>().ok())
    };
    let ((pa, na), (pb, nb)) = (split(&a), split(&b));
    match (na, nb) {
        (Some(x), Some(y)) if pa == pb && !pa.is_empty() => {
            if x == y {
                4
            } else {
                2
            }
        }
        _ => 0,
    }
}

/// The table keys best first for a specimen id, with their scores (ties keep the table order; for the near
/// misses of the same letters the closest number comes first).
pub fn rank_names(id: &str, keys: &[String]) -> Vec<(usize, u8)> {
    let num = |s: &str| -> Option<i64> {
        let d: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
        d.parse().ok()
    };
    let idn = num(id);
    let mut v: Vec<(usize, u8)> = keys.iter().enumerate().map(|(i, k)| (i, name_score(id, k))).collect();
    v.sort_by_key(|&(i, sc)| {
        let dist = match (idn, num(&keys[i])) {
            (Some(a), Some(b)) => (a - b).abs(),
            _ => i64::MAX,
        };
        (std::cmp::Reverse(sc), if sc == 2 { dist } else { 0 }, i)
    });
    v
}

/// Slippy-map tile numbers (fractional) of a longitude / latitude at a zoom level.
pub fn tile_from_lonlat(lon: f64, lat: f64, z: u32) -> (f64, f64) {
    let (mx, my) = merc_from_lonlat(lon, lat);
    tile_from_merc(mx, my, z)
}

/// Size of the Web Mercator world in metres.
pub const WORLD: f64 = 2.0 * PI * WGS_A;

pub fn tile_from_merc(mx: f64, my: f64, z: u32) -> (f64, f64) {
    let n = f64::from(1u32 << z);
    ((mx + WORLD / 2.0) / WORLD * n, (WORLD / 2.0 - my) / WORLD * n)
}

/// Web Mercator metres of the top-left corner of a tile.
pub fn merc_from_tile(tx: f64, ty: f64, z: u32) -> (f64, f64) {
    let n = f64::from(1u32 << z);
    (tx / n * WORLD - WORLD / 2.0, WORLD / 2.0 - ty / n * WORLD)
}

/// The tile zoom level whose pixels are closest to the screen's metres per point.
pub fn zoom_for(metres_per_point: f64, pixels_per_point: f32) -> u32 {
    let m_per_px = metres_per_point / f64::from(pixels_per_point.max(0.5));
    // a 256 px tile covers WORLD / 2^z metres
    let z = (WORLD / 256.0 / m_per_px.max(1e-6)).log2().round();
    z.clamp(0.0, 19.0) as u32
}

/// Foliation and lineation of an AMS fabric as they are drawn on a map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fabric {
    /// Strike of the magnetic foliation (right-hand rule, degrees 0-360), dip direction and dip (0-90).
    pub strike: f64,
    pub dip_dir: f64,
    pub dip: f64,
    /// Trend and plunge of K1 (the lineation), degrees; plunge 0-90 (axes are lines, shown as the downward end).
    pub trend: f64,
    pub plunge: f64,
}

fn norm360(a: f64) -> f64 {
    ((a % 360.0) + 360.0) % 360.0
}

/// Fabric from K1 and K3 as (declination, inclination) in degrees. The foliation is the plane perpendicular to K3.
pub fn fabric_from_axes(k1: (f64, f64), k3: (f64, f64)) -> Fabric {
    let down = |(d, i): (f64, f64)| if i < 0.0 { (norm360(d + 180.0), -i) } else { (norm360(d), i) };
    let (t, p) = down(k1);
    let (d3, i3) = down(k3);
    let dip_dir = norm360(d3 + 180.0);
    Fabric { strike: norm360(dip_dir - 90.0), dip_dir, dip: 90.0 - i3, trend: t, plunge: p }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn mercator_round_trip_and_known_points() {
        let (x0, y0) = merc_from_lonlat(0.0, 0.0);
        assert!(close(x0, 0.0, 1e-6) && close(y0, 0.0, 1e-6));
        let (x, y) = merc_from_lonlat(180.0, 0.0);
        assert!(close(x, WORLD / 2.0, 1e-6) && close(y, 0.0, 1e-6));
        for (lo, la) in [(-2.8, 56.34), (67.5, 50.2), (-150.0, -33.0)] {
            let (x, y) = merc_from_lonlat(lo, la);
            let (lo2, la2) = lonlat_from_merc(x, y);
            assert!(close(lo, lo2, 1e-9) && close(la, la2, 1e-9));
        }
    }

    #[test]
    fn utm_matches_an_independent_calculation_and_round_trips() {
        // central meridian of zone 31 at the equator
        let (e, n) = utm_from_lonlat(3.0, 0.0, 31, true);
        assert!(close(e, 500_000.0, 1e-6) && close(n, 0.0, 1e-6));
        // 3 deg east of the central meridian of zone 38 (47 E) at 50 N: values from Snyder's classic series
        // (independent of the Krueger series used here), see utm_reference() below
        let (e, n) = utm_from_lonlat(50.0, 50.0, 38, true);
        let (re, rn) = utm_reference(50.0, 50.0, 38);
        assert!(close(e, re, 0.01) && close(n, rn, 0.01), "{e} {n} vs {re} {rn}");
        for (lo, la, z, north) in [(67.5, 50.2, 42u8, true), (-2.8, 56.34, 30, true), (151.2, -33.9, 56, false), (-70.0, -33.0, 19, false)] {
            let (e, n) = utm_from_lonlat(lo, la, z, north);
            let (lo2, la2) = lonlat_from_utm(e, n, z, north);
            assert!(close(lo, lo2, 1e-9) && close(la, la2, 1e-9), "{lo} {la} -> {lo2} {la2}");
        }
    }

    /// Snyder (1987) transverse Mercator series, WGS 84, k0 = 0.9996.
    fn utm_reference(lon: f64, lat: f64, zone: u8) -> (f64, f64) {
        let a = WGS_A;
        let e2 = WGS_F * (2.0 - WGS_F);
        let ep2 = e2 / (1.0 - e2);
        let phi = lat.to_radians();
        let lam0 = (f64::from(zone) * 6.0 - 183.0).to_radians();
        let nn = a / (1.0 - e2 * phi.sin().powi(2)).sqrt();
        let t = phi.tan().powi(2);
        let c = ep2 * phi.cos().powi(2);
        let aa = (lon.to_radians() - lam0) * phi.cos();
        let m = a
            * ((1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0) * phi
                - (3.0 * e2 / 8.0 + 3.0 * e2 * e2 / 32.0 + 45.0 * e2.powi(3) / 1024.0) * (2.0 * phi).sin()
                + (15.0 * e2 * e2 / 256.0 + 45.0 * e2.powi(3) / 1024.0) * (4.0 * phi).sin()
                - (35.0 * e2.powi(3) / 3072.0) * (6.0 * phi).sin());
        let e = 500_000.0
            + K0 * nn * (aa + (1.0 - t + c) * aa.powi(3) / 6.0 + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * aa.powi(5) / 120.0);
        let n = K0
            * (m + nn
                * phi.tan()
                * (aa * aa / 2.0 + (5.0 - t + 9.0 * c + 4.0 * c * c) * aa.powi(4) / 24.0 + (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) * aa.powi(6) / 720.0));
        (e, n)
    }

    #[test]
    fn epsg_codes_and_prj_text() {
        assert_eq!(Crs::from_epsg(4326), Some(Crs::LonLat));
        assert_eq!(Crs::from_epsg(32638), Some(Crs::Utm { zone: 38, north: true }));
        assert_eq!(Crs::from_epsg(32742), Some(Crs::Utm { zone: 42, north: false }));
        assert_eq!(Crs::from_epsg(25832), Some(Crs::Utm { zone: 32, north: true }));
        assert_eq!(Crs::from_prj("PROJCS[\"WGS_1984_UTM_Zone_38N\",GEOGCS[\"GCS_WGS_1984\"],PROJECTION[\"Transverse_Mercator\"]]"), Some(Crs::Utm { zone: 38, north: true }));
        assert_eq!(Crs::from_prj("PROJCS[\"WGS 84 / UTM zone 42S\",AUTHORITY[\"EPSG\",\"32742\"]]"), Some(Crs::Utm { zone: 42, north: false }));
        assert_eq!(Crs::from_prj("GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\"]]"), Some(Crs::LonLat));
    }

    #[test]
    fn many_systems_through_the_registry() {
        // Lambert-93: the projection origin 3 E, 46.5 N is at (700000, 6600000)
        let c = Crs::from_epsg(2154).unwrap();
        let (lo, la) = c.to_lonlat(700_000.0, 6_600_000.0);
        assert!(close(lo, 3.0, 1e-6) && close(la, 46.5, 1e-6), "{lo} {la}");
        // ETRS89 / LAEA Europe: the origin 10 E, 52 N is at (4321000, 3210000)
        let c = Crs::from_epsg(3035).unwrap();
        let (lo, la) = c.to_lonlat(4_321_000.0, 3_210_000.0);
        assert!(close(lo, 10.0, 1e-6) && close(la, 52.0, 1e-6), "{lo} {la}");
        // Pulkovo 1942 / Gauss-Krueger zone 12: the central meridian 69 E has x = 12500000 (zone number in front)
        let c = Crs::from_epsg(28412).unwrap();
        let (lo, la) = c.to_lonlat(12_500_000.0, 5_600_000.0);
        assert!(close(lo, 69.0, 0.01) && la > 50.0 && la < 51.5, "{lo} {la}");
        let (x, y) = c.from_lonlat(lo, la);
        assert!(close(x, 12_500_000.0, 1e-3) && close(y, 5_600_000.0, 1e-3), "{x} {y}");
        // British National Grid (Helmert datum shift, a few metres)
        let c = Crs::from_epsg(27700).unwrap();
        let (lo, la) = c.to_lonlat(651_409.9, 313_177.3);
        assert!(close(lo, 1.7179, 0.003) && close(la, 52.6576, 0.003), "{lo} {la}");
        // UTM through the registry agrees with the built-in one (NAD83 / UTM 14N is built in, WGS 84 / UTM 14N via proj4rs)
        let via = Crs::Epsg(32614);
        let (lo, la) = via.to_lonlat(500_000.0, 5_000_000.0);
        let (lo2, la2) = Crs::Utm { zone: 14, north: true }.to_lonlat(500_000.0, 5_000_000.0);
        assert!(close(lo, lo2, 1e-7) && close(la, la2, 1e-7), "{lo} {la} / {lo2} {la2}");
        // unknown codes
        assert_eq!(Crs::from_epsg(1), None);
        assert_eq!(Crs::from_epsg(999_999), None);
    }

    #[test]
    fn typed_definitions_and_search() {
        assert_eq!(Crs::from_definition("EPSG:32642"), Ok(Crs::Utm { zone: 42, north: true }));
        assert_eq!(Crs::from_definition(" 4326 "), Ok(Crs::LonLat));
        assert_eq!(Crs::from_definition("28412").unwrap(), Crs::Epsg(28412));
        assert!(Crs::from_definition("").is_err() && Crs::from_definition("abc").is_err() && Crs::from_definition("EPSG:1").is_err());
        let c = Crs::from_definition("+proj=tmerc +lat_0=0 +lon_0=69 +k=1 +x_0=500000 +y_0=0 +ellps=krass +units=m +no_defs").unwrap();
        let (lo, la) = c.to_lonlat(500_000.0, 5_600_000.0);
        assert!(close(lo, 69.0, 1e-6) && la > 50.0);
        assert_eq!(Crs::from_definition("+proj=tmerc +lat_0=0 +lon_0=69 +k=1 +x_0=500000 +y_0=0 +ellps=krass +units=m +no_defs").unwrap(), c);
        assert!(Crs::from_definition("+proj=nonsense").is_err());
        // search by words and by code
        let hits = epsg_search("pulkovo gauss zone 12", 5);
        assert!(hits.iter().any(|h| h.0 == 28412 || h.0 == 28412), "{hits:?}");
        assert_eq!(epsg_search("27700", 3)[0].0, 27700);
        assert!(epsg_search("british national grid", 5).iter().any(|h| h.0 == 27700));
        assert!(epsg_name(27700).unwrap().contains("British National Grid"));
        assert!(Crs::Epsg(27700).label().contains("EPSG:27700"));
    }

    #[test]
    fn prj_names_from_esri_are_matched_against_the_registry() {
        assert_eq!(Crs::from_prj("PROJCS[\"Pulkovo_1942_GK_Zone_13\",GEOGCS[\"GCS_Pulkovo_1942\"],PROJECTION[\"Gauss_Kruger\"]]"), Some(Crs::Epsg(28413)));
        assert_eq!(Crs::from_prj("PROJCS[\"British_National_Grid\",GEOGCS[\"GCS_OSGB_1936\"]]"), Some(Crs::Epsg(27700)));
        assert_eq!(Crs::from_prj("PROJCS[\"NAD_1983_UTM_Zone_14N\",GEOGCS[\"GCS_North_American_1983\"]]"), Some(Crs::Utm { zone: 14, north: true }));
        assert_eq!(Crs::from_prj("PROJCS[\"Something_Unknown_Grid\"]"), None);
    }

    #[test]
    fn two_points_give_a_similarity_and_many_points_a_least_squares_affine() {
        // a map rotated by 30 degrees, 2 m per pixel, y pixels down
        let (s, c) = (0.5f64, 30f64.to_radians().cos());
        let truth = Affine { a: 2.0 * c, b: 2.0 * s, c: 1000.0, d: 2.0 * s, e: -2.0 * c, f: 5000.0 };
        let pts: Vec<ControlPoint> = [(0.0, 0.0), (400.0, 50.0), (350.0, 600.0), (20.0, 580.0)].iter().map(|&(x, y)| ((x, y), truth.apply(x, y))).collect();
        let two = fit_affine(&pts[..2]).unwrap();
        for (k, v) in [(two.a, truth.a), (two.b, truth.b), (two.c, truth.c), (two.d, truth.d), (two.e, truth.e), (two.f, truth.f)] {
            assert!(close(k, v, 1e-6), "{k} {v}");
        }
        let all = fit_affine(&pts).unwrap();
        for (k, v) in [(all.a, truth.a), (all.b, truth.b), (all.c, truth.c), (all.d, truth.d), (all.e, truth.e), (all.f, truth.f)] {
            assert!(close(k, v, 1e-6), "{k} {v}");
        }
        let (r, rms) = residuals(&pts, &all);
        assert!(rms < 1e-6 && r.iter().all(|x| *x < 1e-6));
        // inverse
        let inv = all.inverse().unwrap();
        let (x, y) = all.apply(123.0, 456.0);
        let (px, py) = inv.apply(x, y);
        assert!(close(px, 123.0, 1e-6) && close(py, 456.0, 1e-6));
    }

    #[test]
    fn degenerate_control_points_are_refused() {
        assert!(fit_affine(&[((0.0, 0.0), (0.0, 0.0))]).is_err());
        assert!(fit_affine(&[((5.0, 5.0), (0.0, 0.0)), ((5.0, 5.0), (1.0, 1.0))]).is_err());
        let line = [((0.0, 0.0), (0.0, 0.0)), ((1.0, 1.0), (1.0, 1.0)), ((2.0, 2.0), (2.0, 2.0))];
        assert!(fit_affine(&line).is_err());
    }

    #[test]
    fn world_file_is_corner_referenced() {
        // 10 m pixels, top-left pixel centre at (1005, 2995): the top-left corner is (1000, 3000)
        let t = parse_world_file("10\n0\n0\n-10\n1005\n2995\n").unwrap();
        let (x, y) = t.apply(0.0, 0.0);
        assert!(close(x, 1000.0, 1e-9) && close(y, 3000.0, 1e-9));
        let (x, y) = t.apply(100.0, 200.0);
        assert!(close(x, 2000.0, 1e-9) && close(y, 1000.0, 1e-9));
        assert!(parse_world_file("1\n2\n3").is_none());
        assert!(parse_world_file("0\n0\n0\n0\n5\n5\n").is_none());
    }

    #[test]
    fn angles_typed_in_a_table() {
        assert_eq!(parse_angle("56.34"), Some(56.34));
        assert_eq!(parse_angle(" 56,34 "), Some(56.34));
        assert_eq!(parse_angle("-2.8"), Some(-2.8));
        assert!(close(parse_angle("56°20'24\"N").unwrap(), 56.34, 1e-9));
        assert!(close(parse_angle("56 20.4 N").unwrap(), 56.34, 1e-9));
        assert!(close(parse_angle("2°48'00\"W").unwrap(), -2.8, 1e-9));
        assert!(close(parse_angle("33 S").unwrap(), -33.0, 1e-9));
        assert!(close(parse_angle("-33°30'").unwrap(), -33.5, 1e-9));
        assert_eq!(parse_angle("12 34 56 78"), None);
        assert_eq!(parse_angle("abc"), None);
        assert_eq!(parse_angle(""), None);
        assert_eq!(parse_angle("NaN"), None);
        // an easting with a thousands space is a plain number too
        assert_eq!(parse_angle("512345.6"), Some(512345.6));
    }

    #[test]
    fn csv_with_quotes_semicolons_and_bom() {
        let g = grid_from_csv("\u{feff}Name;Lat;Lon\r\nB1;\"56,34\";-2,8\r\n\r\n");
        assert_eq!(g, vec![vec!["Name", "Lat", "Lon"], vec!["B1", "56,34", "-2,8"]]);
        let g = grid_from_csv("a,b\n\"x, y\",\"say \"\"hi\"\"\"\n");
        assert_eq!(g[1], vec!["x, y", "say \"hi\""]);
        assert_eq!(grid_from_csv("a\tb\n1\t2")[1], vec!["1", "2"]);
        // columns separated by spaces (a pasted table)
        let g = grid_from_csv("EAST    NORTH\n412345.678    4851234.567\n  412341.044 4851437.89\n");
        assert_eq!(g, vec![vec!["EAST", "NORTH"], vec!["412345.678", "4851234.567"], vec!["412341.044", "4851437.89"]]);
        assert!(grid_from_csv("").is_empty());
    }

    #[test]
    fn names_match_exactly_or_by_block() {
        let keys: Vec<String> = ["B1", "B10", "B1-2", "C3"].iter().map(|s| s.to_string()).collect();
        let sp: Vec<String> = ["b1", "B1-1", "B1-2", "B10-2", "B1a", "B2", "C3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(match_names(&sp, &keys, MatchMode::Exact), vec![Some(0), None, Some(2), None, None, None, Some(3)]);
        // block mode: B1-2 stays on its own row (exact), B10-2 goes to B10, B1a to B1, B2 has none
        assert_eq!(match_names(&sp, &keys, MatchMode::StartsWithKey), vec![Some(0), Some(0), Some(2), Some(1), Some(0), None, Some(3)]);
        // no names: the n-th specimen belongs to the n-th row
        assert_eq!(match_names(&sp, &keys[..3], MatchMode::RowOrder), vec![Some(0), Some(1), Some(2), None, None, None, None]);
    }

    #[test]
    fn typed_coordinates_are_recognised() {
        // decimal degrees, the written order latitude, longitude (and a note that it was assumed)
        let d = detect_crs("47.12345", "10.54321", 44, true, None).unwrap();
        assert_eq!((d.crs, d.x, d.y, d.swapped), (Crs::LonLat, 10.54321, 47.12345, true));
        assert!(d.what.contains("assumed"));
        // a longitude above 90 settles the order, and so do hemisphere letters
        // both numbers below 90 fit either way round: the written order is assumed and the user is told
        let d = detect_crs("10.54321", "47.12345", 44, true, None).unwrap();
        assert_eq!((d.x, d.y, d.swapped), (47.12345, 10.54321, true));
        assert!(d.what.contains("assumed"));
        let d = detect_crs("100.5", "47.8", 44, true, None).unwrap();
        assert_eq!((d.x, d.y, d.swapped), (100.5, 47.8, false));
        let d = detect_crs("10°32'35\"E", "47°07'24\"N", 44, true, None).unwrap();
        assert!(close(d.x, 10.543056, 1e-4) && close(d.y, 47.123333, 1e-4) && !d.swapped && !d.what.contains("assumed"), "{d:?}");
        // decimal comma
        assert_eq!(detect_crs("46,80242", "81,42206", 44, true, None).unwrap().crs, Crs::LonLat);
        // UTM: the zone is a guess unless the previous specimen settled it
        let d = detect_crs("412345.678", "4851234.567", 33, true, None).unwrap();
        assert_eq!(d.crs, Crs::Utm { zone: 33, north: true });
        assert!(d.what.contains("not in the numbers"));
        let d = detect_crs("412345.678", "4851234.567", 33, true, Some(Crs::Utm { zone: 31, north: false })).unwrap();
        assert_eq!(d.crs, Crs::Utm { zone: 31, north: false });
        // northing typed first
        let d = detect_crs("4851234.567", "412345.678", 33, true, None).unwrap();
        assert_eq!((d.x, d.y, d.swapped), (412345.678, 4851234.567, true));
        // Gauss-Krueger with the zone number in front, as on Soviet maps
        let d = detect_crs("14412345", "5300123", 44, true, None).unwrap();
        assert_eq!(d.crs, Crs::Epsg(28414));
        let (lo, la) = d.crs.to_lonlat(d.x, d.y);
        assert!(close(lo, 79.83, 0.15) && close(la, 47.85, 0.15), "{lo} {la}");
        // nothing sensible: an error, or the previous system when there is one
        assert!(detect_crs("9999999999", "12", 44, true, None).is_err());
        assert!(detect_crs("abc", "12", 44, true, None).is_err());
        assert!(detect_crs("95", "200", 44, true, None).is_err());
        // the previous system settles ambiguous metres: 4321000, 3210000 would read as Gauss-Krueger zone 4 on its own
        assert_eq!(detect_crs("4321000", "3210000", 44, true, None).unwrap().crs, Crs::Epsg(28404));
        assert_eq!(detect_crs("4321000", "3210000", 44, true, Some(Crs::Epsg(3035))).unwrap().crs, Crs::Epsg(3035));
    }

    #[test]
    fn specimen_ids_are_ranked_against_the_table_names() {
        assert_eq!(name_score("XQ77RS002", "xq77rs002"), 6);
        assert_eq!(name_score("XQ77RS002", "XQ77-RS002"), 6);
        assert_eq!(name_score("XQ77RS002", "RS002"), 5);
        assert_eq!(name_score("XQ77RS002", "XQ77RS2"), 4);
        assert_eq!(name_score("XQ77RS002", "XQ77RS003"), 2);
        assert_eq!(name_score("XQ77RS002", "XY77"), 0);
        assert_eq!(name_score("", "A"), 0);
        let keys: Vec<String> = ["XQ77RS005", "XQ77RS003", "XQ77RS002", "RS02x", "QQ"].iter().map(|s| s.to_string()).collect();
        let r = rank_names("XQ77RS002", &keys);
        assert_eq!(r[0], (2, 6));
        // the near misses: the closest number first (003 before 005)
        assert_eq!(r[1].0, 1);
        assert_eq!(r[2].0, 0);
        assert_eq!(r.last().unwrap().0, 4);
    }

    #[test]
    fn tiles() {
        assert_eq!(tile_from_lonlat(0.0, 0.0, 1), (1.0, 1.0));
        let (tx, ty) = tile_from_lonlat(-180.0, MAX_LAT, 3);
        assert!(close(tx, 0.0, 1e-9) && close(ty, 0.0, 1e-6));
        let (mx, my) = merc_from_tile(2.0, 3.0, 3);
        let (tx, ty) = tile_from_merc(mx, my, 3);
        assert!(close(tx, 2.0, 1e-9) && close(ty, 3.0, 1e-9));
        // one tile pixel of zoom 0 is WORLD / 256 metres
        assert_eq!(zoom_for(WORLD / 256.0, 1.0), 0);
        assert_eq!(zoom_for(WORLD / 256.0 / 1024.0, 1.0), 10);
    }

    #[test]
    fn fabric_from_k1_and_k3() {
        // K3 pole plunging 20 deg to the west (270): the plane dips 70 deg to the east (090), strikes 000
        let f = fabric_from_axes((10.0, 5.0), (270.0, 20.0));
        assert!(close(f.dip, 70.0, 1e-9) && close(f.dip_dir, 90.0, 1e-9) && close(f.strike, 0.0, 1e-9));
        assert!(close(f.trend, 10.0, 1e-9) && close(f.plunge, 5.0, 1e-9));
        // axes are lines: an upward K3 (negative inclination) gives the same plane as its downward end
        let g = fabric_from_axes((190.0, -5.0), (90.0, -20.0));
        assert!(close(g.dip, 70.0, 1e-9) && close(g.dip_dir, 90.0, 1e-9));
        assert!(close(g.trend, 10.0, 1e-9) && close(g.plunge, 5.0, 1e-9));
        // vertical K3 = horizontal foliation
        assert!(close(fabric_from_axes((0.0, 0.0), (0.0, 90.0)).dip, 0.0, 1e-9));
    }

    #[test]
    fn georef_chain_for_utm_and_web_mercator() {
        // a UTM 42N map with 100 m pixels, top-left corner at (500000, 5600000)
        let g = Georef { affine: Affine { a: 100.0, b: 0.0, c: 500_000.0, d: 0.0, e: -100.0, f: 5_600_000.0 }, crs: Crs::Utm { zone: 42, north: true } };
        let (mx, my) = g.pixel_to_merc(30.0, 70.0);
        let (px, py) = g.merc_to_pixel(mx, my).unwrap();
        assert!(close(px, 30.0, 1e-4) && close(py, 70.0, 1e-4));
        let (lo, la) = lonlat_from_merc(mx, my);
        let (e, n) = utm_from_lonlat(lo, la, 42, true);
        assert!(close(e, 503_000.0, 1e-3) && close(n, 5_593_000.0, 1e-3));
    }
}
