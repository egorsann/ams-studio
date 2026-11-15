//! The orientation table for "Spec to Geo (Owens) from file": one row per specimen with the strike and dip of a plane
//! in the field and of the same plane on the drilling table (FS, FD, DS, DD of the Owens window).
//!
//! Columns are found by their header, so the lab's own sheet can be used as it is:
//! * specimen: `Specimen`, `Sample ID`, `Name`, `Block Sample`, ...
//! * field strike: `FS`, `Field strike`, `Strike`; with `LHR` in the header it is a left-hand-rule strike,
//!   and `Dip Direction` can be given instead of a strike
//! * field dip: `FD`, `Field dip`, `Dip`
//! * drilling table: `DS`, `Drill table strike`, `DD`, `Drill table dip`
//!
//! The strike convention of the lab (to be confirmed by the lab): the index finger along the strike, the thumb
//! towards the dip direction, so a strike of 0 deg (north) dips towards 90 deg. That is the same as the numbers the
//! Owens rotation uses (dip direction = strike + 90), so a column headed `LHR` is used as it stands, and a dip
//! direction has 90 degrees taken off. The convention of the field strike column is also applied to the drill table
//! strike. (A textbook left-hand-rule strike would be 180 degrees away; change [`StrikeKind::to_rhr`] if the lab
//! does mean that.)

/// The role of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Specimen,
    FieldStrike,
    FieldDip,
    TableStrike,
    TableDip,
}

pub const ROLES: [Role; 5] = [Role::Specimen, Role::FieldStrike, Role::FieldDip, Role::TableStrike, Role::TableDip];

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Specimen => "Specimen",
            Role::FieldStrike => "Strike (field)",
            Role::FieldDip => "Dip (field)",
            Role::TableStrike => "Drill table strike",
            Role::TableDip => "Drill table dip",
        }
    }
}

/// How strikes are given in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeKind {
    RightHand,
    LeftHand,
    DipDirection,
}

impl StrikeKind {
    /// Right-hand-rule strike for a value in this convention.
    pub fn to_rhr(self, v: f64) -> f64 {
        let x = match self {
            StrikeKind::RightHand => v,
            // the lab's left-hand rule as described (thumb to the dip direction): the same numbers
            StrikeKind::LeftHand => v,
            StrikeKind::DipDirection => v - 90.0,
        };
        x.rem_euclid(360.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    /// FS, FD, DS, DD as used by the Owens rotation (right-hand-rule strikes).
    pub vals: [f64; 4],
    /// The numbers as written in the file, for display.
    pub raw: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub rows: Vec<Row>,
    /// Header text found for each role (index = position in [`ROLES`]).
    pub found: [Option<String>; 5],
    pub strike_kind: StrikeKind,
    /// 1-based number of the header row in the sheet.
    pub header_row: usize,
    /// Data rows skipped because a number was missing or unreadable.
    pub skipped: usize,
}

/// What was recognised when the table could not be used.
#[derive(Debug, Clone, PartialEq)]
pub struct TableError {
    pub message: String,
    pub found: [Option<String>; 5],
}

fn norm(h: &str) -> String {
    h.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Role and strike convention of a header, if it is one we know.
fn classify(h: &str) -> Option<(Role, Option<StrikeKind>)> {
    let raw = h.to_lowercase();
    let n = norm(h);
    if n.is_empty() {
        return None;
    }
    let drill = n.contains("drill") || n.contains("table");
    let is_dd = n.contains("dipdir") || n.contains("dipdirection");
    let kind = if n.contains("lhr") || n.contains("lefthand") { StrikeKind::LeftHand } else if is_dd { StrikeKind::DipDirection } else { StrikeKind::RightHand };
    if matches!(n.as_str(), "specimen" | "sample" | "sampleid" | "name" | "blocksample" | "blocksamplesubspec" | "id" | "specimenid" | "record") || n.starts_with("blocksample") {
        return Some((Role::Specimen, None));
    }
    if matches!(n.as_str(), "fs" | "fieldstrike") || (n.contains("strike") && !drill && !raw.contains("core")) || (is_dd && !drill) {
        return Some((Role::FieldStrike, Some(kind)));
    }
    if matches!(n.as_str(), "fd" | "fielddip") || (n == "dip") || (n.starts_with("dip") && !drill && !is_dd && !n.contains("direction")) {
        return Some((Role::FieldDip, None));
    }
    if n == "ds" || (drill && n.contains("strike")) {
        return Some((Role::TableStrike, Some(kind)));
    }
    if n == "dd" || (drill && n.contains("dip")) {
        return Some((Role::TableDip, None));
    }
    None
}

fn number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Reads a table from rows of cells (a sheet or a csv). The header row is the first of the first 20 rows that
/// names a specimen column, a field dip, and the drill table strike and dip.
pub fn from_grid(grid: &[Vec<String>]) -> Result<Table, TableError> {
    let mut best: Option<(usize, [Option<usize>; 5], StrikeKind, [Option<String>; 5])> = None;
    let mut best_found: [Option<String>; 5] = Default::default();
    for (ri, row) in grid.iter().enumerate().take(20) {
        let mut cols: [Option<usize>; 5] = [None; 5];
        let mut heads: [Option<String>; 5] = Default::default();
        let mut kind = StrikeKind::RightHand;
        for (ci, cell) in row.iter().enumerate() {
            if let Some((role, k)) = classify(cell) {
                let ix = ROLES.iter().position(|r| *r == role).unwrap();
                if cols[ix].is_none() {
                    cols[ix] = Some(ci);
                    heads[ix] = Some(cell.trim().to_string());
                    if let (Role::FieldStrike, Some(k)) = (role, k) {
                        kind = k;
                    }
                }
            }
        }
        let count = cols.iter().flatten().count();
        if best_found.iter().flatten().count() < count {
            best_found = heads.clone();
        }
        if cols.iter().all(|c| c.is_some()) {
            best = Some((ri, cols, kind, heads));
            break;
        }
    }
    let Some((hr, cols, kind, found)) = best else {
        let missing: Vec<&str> = ROLES.iter().enumerate().filter(|(i, _)| best_found[*i].is_none()).map(|(_, r)| r.label()).collect();
        return Err(TableError { message: format!("These columns were not found in the file: {}.", missing.join(", ")), found: best_found });
    };
    let cell = |row: &Vec<String>, c: usize| row.get(c).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut rows = vec![];
    let mut skipped = 0;
    for row in grid.iter().skip(hr + 1) {
        let name = cell(row, cols[0].unwrap());
        if name.is_empty() && row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let nums: Vec<Option<f64>> = (1..5).map(|k| number(&cell(row, cols[k].unwrap()))).collect();
        if name.is_empty() || nums.iter().any(|n| n.is_none()) {
            skipped += 1;
            continue;
        }
        let raw = [nums[0].unwrap(), nums[1].unwrap(), nums[2].unwrap(), nums[3].unwrap()];
        rows.push(Row { name, vals: [kind.to_rhr(raw[0]), raw[1], kind.to_rhr(raw[2]), raw[3]], raw });
    }
    if rows.is_empty() {
        return Err(TableError { message: "The columns were found but no row has all four numbers.".into(), found });
    }
    Ok(Table { rows, found, strike_kind: kind, header_row: hr + 1, skipped })
}

/// A csv (comma or semicolon separated) as a grid.
pub fn grid_from_csv(text: &str) -> Vec<Vec<String>> {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else if first.matches('\t').count() > first.matches(',').count() { '\t' } else { ',' };
    text.lines().map(|l| l.split(sep).map(|c| c.trim().trim_matches('"').to_string()).collect()).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// For every record of a file: the row of the table that gives its values and how it was found.
/// A row naming the record applies to it; otherwise a row naming the file (one of `ids`: file name without `.ams`,
/// folder name, name without the main-file identifier) applies to all its records; a table with a single row
/// applies to everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Record,
    Specimen,
    OnlyRow,
}

pub fn assign(t: &Table, records: &[String], ids: &[String]) -> Vec<Option<(usize, Via)>> {
    let by_id = ids.iter().filter(|i| !i.is_empty()).find_map(|id| t.rows.iter().position(|r| same(&r.name, id)));
    records
        .iter()
        .map(|rec| {
            if let Some(i) = t.rows.iter().position(|r| same(&r.name, rec)) {
                Some((i, Via::Record))
            } else if let Some(i) = by_id {
                Some((i, Via::Specimen))
            } else if t.rows.len() == 1 {
                Some((0, Via::OnlyRow))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn plain_headers() {
        let t = from_grid(&g(&[&["Specimen", "FS", "FD", "DS", "DD"], &["ZZ01", "30", "20", "45", "10"], &["", "", "", "", ""], &["ZZ02", "100,5", "35", "10", "5"]])).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].vals, [30.0, 20.0, 45.0, 10.0]);
        assert_eq!(t.rows[1].vals[0], 100.5);
        assert_eq!(t.strike_kind, StrikeKind::RightHand);
    }

    #[test]
    fn the_lab_template_headers_and_a_title_row() {
        let t = from_grid(&g(&[
            &["MUST COMPLETE", "", "", "", ""],
            &["Sample ID", "Strike (LHR)", "Dip", "Drill table strike ", "Drill table dip"],
            &["RR25RM001", "10", "40", "350", "30"],
        ]))
        .unwrap();
        assert_eq!(t.header_row, 2);
        assert_eq!(t.strike_kind, StrikeKind::LeftHand);
        // the lab's left-hand rule (thumb to the dip direction) gives the numbers the rotation uses
        assert_eq!(t.rows[0].vals, [10.0, 40.0, 350.0, 30.0]);
        assert_eq!(t.rows[0].raw, [10.0, 40.0, 350.0, 30.0]);
    }

    #[test]
    fn dip_direction_input() {
        let t = from_grid(&g(&[&["Name", "Dip Direction", "Dip", "DS", "DD"], &["A", "100", "30", "10", "5"]])).unwrap();
        assert_eq!(t.strike_kind, StrikeKind::DipDirection);
        assert_eq!(t.rows[0].vals[0], 10.0);
        assert_eq!(t.rows[0].vals[2], 280.0);
    }

    #[test]
    fn missing_columns_are_named() {
        let e = from_grid(&g(&[&["Specimen", "FS", "FD"], &["A", "1", "2"]])).unwrap_err();
        assert!(e.message.contains("Drill table strike") && e.message.contains("Drill table dip"));
        assert!(e.found[0].is_some() && e.found[3].is_none());
        let e = from_grid(&g(&[&["Specimen", "FS", "FD", "DS", "DD"], &["A", "x", "2", "3", "4"]])).unwrap_err();
        assert!(e.message.contains("no row"));
    }

    #[test]
    fn csv_with_semicolons() {
        let grid = grid_from_csv("Specimen;FS;FD;DS;DD\nA;1,5;2;3;4\n");
        let t = from_grid(&grid).unwrap();
        assert_eq!(t.rows[0].vals, [1.5, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn rows_are_assigned_to_records() {
        let t = from_grid(&g(&[&["Specimen", "FS", "FD", "DS", "DD"], &["ZZ01-A1", "1", "2", "3", "4"], &["ZZ01", "5", "6", "7", "8"], &["ZZ09", "9", "9", "9", "9"]])).unwrap();
        let rec = vec!["ZZ01-A1".to_string(), "ZZ01-B1".to_string()];
        let a = assign(&t, &rec, &["ZZ01_use".into(), "ZZ01".into()]);
        assert_eq!(a[0], Some((0, Via::Record)));
        assert_eq!(a[1], Some((1, Via::Specimen)));
        assert_eq!(assign(&t, &rec, &["nothing".into()])[1], None);
        let one = from_grid(&g(&[&["Specimen", "FS", "FD", "DS", "DD"], &["whatever", "1", "2", "3", "4"]])).unwrap();
        assert_eq!(assign(&one, &rec, &[])[1], Some((0, Via::OnlyRow)));
    }
}
