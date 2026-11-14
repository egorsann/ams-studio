//! `.ged` (geological data) files written by the GED Editor of Anisoft 6.
//!
//! Fixed-width CRLF text. Line 1 (62 columns): row-count field (4, left aligned; the
//! original writes `rows + 2`), site name (16), latitude (7), longitude (7), rock type,
//! stratigraphy, lithology, region (4 each), OP1..OP4 (3 each). Then one 62-column line
//! per specimen: name (12), sampling dec (4), inc (4), "fol1 code, lin1 code" as two
//! characters (4), fol1 dec/dip (4 each), lin1 trend/plunge (4 each), "fol2 code,
//! lin2 code" (4), fol2 dec/dip, lin2 trend/plunge.

#[derive(Debug, Clone, PartialEq)]
pub struct GedRow {
    pub name: String,
    pub sampling: [f64; 2],
    /// Codes of foliation 1, lineation 1, foliation 2, lineation 2 (one character each, '0' when empty).
    pub codes: [char; 4],
    pub fol1: [f64; 2],
    pub lin1: [f64; 2],
    pub fol2: [f64; 2],
    pub lin2: [f64; 2],
}

impl GedRow {
    pub fn new(name: &str) -> GedRow {
        GedRow { name: name.to_string(), sampling: [0.0; 2], codes: ['0'; 4], fol1: [0.0; 2], lin1: [0.0; 2], fol2: [0.0; 2], lin2: [0.0; 2] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GedFile {
    pub site: String,
    pub latitude: String,
    pub longitude: String,
    /// Rock type, stratigraphy, lithology, region.
    pub rock: [String; 4],
    pub op: [u16; 4],
    pub rows: Vec<GedRow>,
}

impl Default for GedFile {
    fn default() -> Self {
        GedFile { site: String::new(), latitude: String::new(), longitude: String::new(), rock: Default::default(), op: [6, 0, 6, 0], rows: vec![] }
    }
}

pub const LINE_LEN: usize = 62;

fn fit(s: &str, w: usize) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).take(w).collect();
    format!("{t:<w$}")
}

fn num(v: f64, w: usize) -> String {
    // shortest decimal text that fits in the column minus the separating space
    let mut s = format!("{v}");
    if s.len() >= w {
        for dec in (0..4).rev() {
            s = format!("{v:.dec$}");
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            if s.len() < w {
                break;
            }
        }
    }
    fit(&s, w)
}

impl GedFile {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let head = format!(
            "{}{}{}{}{}{}{}{}{}{}{}{}",
            fit(&(self.rows.len() + 2).to_string(), 4),
            fit(&self.site, 16),
            fit(&self.latitude, 7),
            fit(&self.longitude, 7),
            fit(&self.rock[0], 4),
            fit(&self.rock[1], 4),
            fit(&self.rock[2], 4),
            fit(&self.rock[3], 4),
            fit(&self.op[0].to_string(), 3),
            fit(&self.op[1].to_string(), 3),
            fit(&self.op[2].to_string(), 3),
            fit(&self.op[3].to_string(), 3),
        );
        out += &fit(&head, LINE_LEN);
        out += "\r\n";
        for r in &self.rows {
            let codes1: String = [r.codes[0], r.codes[1]].iter().collect();
            let codes2: String = [r.codes[2], r.codes[3]].iter().collect();
            let line = format!(
                "{}{}{}{}{}{}{}{}{}{}{}{}{}",
                fit(&r.name, 12),
                num(r.sampling[0], 4),
                num(r.sampling[1], 4),
                fit(&codes1, 4),
                num(r.fol1[0], 4),
                num(r.fol1[1], 4),
                num(r.lin1[0], 4),
                num(r.lin1[1], 4),
                fit(&codes2, 4),
                num(r.fol2[0], 4),
                num(r.fol2[1], 4),
                num(r.lin2[0], 4),
                num(r.lin2[1], 4),
            );
            out += &fit(&line, LINE_LEN);
            out += "\r\n";
        }
        out
    }

    pub fn parse(text: &str) -> Result<GedFile, String> {
        let mut lines = text.lines();
        let head: Vec<char> = lines.next().ok_or("empty GED file")?.chars().collect();
        let col = |l: &[char], a: usize, w: usize| -> String {
            let end = (a + w).min(l.len());
            if a >= l.len() { String::new() } else { l[a..end].iter().collect::<String>().trim().to_string() }
        };
        let mut g = GedFile {
            site: col(&head, 4, 16),
            latitude: col(&head, 20, 7),
            longitude: col(&head, 27, 7),
            rock: [col(&head, 34, 4), col(&head, 38, 4), col(&head, 42, 4), col(&head, 46, 4)],
            ..Default::default()
        };
        for (i, a) in [50usize, 53, 56, 59].iter().enumerate() {
            g.op[i] = col(&head, *a, 3).parse().unwrap_or(g.op[i]);
        }
        for ln in lines {
            let l: Vec<char> = ln.chars().collect();
            if l.iter().all(|c| c.is_whitespace()) {
                continue;
            }
            let f = |a: usize| col(&l, a, 4).parse::<f64>().unwrap_or(0.0);
            let ch = |a: usize| l.get(a).copied().filter(|c| !c.is_whitespace()).unwrap_or('0');
            g.rows.push(GedRow {
                name: col(&l, 0, 12),
                sampling: [f(12), f(16)],
                codes: [ch(20), ch(21), ch(40), ch(41)],
                fol1: [f(24), f(28)],
                lin1: [f(32), f(36)],
                fol2: [f(44), f(48)],
                lin2: [f(52), f(56)],
            });
        }
        Ok(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // made-up two-row file in the exact layout written by the original GED Editor
    const SAMPLE: &str = "4   AB              11     22     T   S   L   R   6  0  6  0  \r\nC1          30  40  78  101 21  102 22  95  103 23  104 24    \r\nB1          50  60  14  2   3   5   6   71  8   9   2   3     \r\n";

    #[test]
    fn parse_then_write_is_byte_identical() {
        let g = GedFile::parse(SAMPLE).unwrap();
        assert_eq!(g.site, "AB");
        assert_eq!(g.op, [6, 0, 6, 0]);
        assert_eq!(g.rows.len(), 2);
        assert_eq!(g.rows[0].codes, ['7', '8', '9', '5']);
        assert_eq!(g.rows[1].fol1, [2.0, 3.0]);
        assert_eq!(g.rows[1].lin2, [2.0, 3.0]);
        assert_eq!(g.to_text(), SAMPLE);
    }

    #[test]
    fn empty_cells_are_written_as_zero_and_lines_are_62_wide() {
        let mut g = GedFile::default();
        g.rows.push(GedRow::new("ZZ1"));
        let t = g.to_text();
        for l in t.split("\r\n").filter(|l| !l.is_empty()) {
            assert_eq!(l.len(), LINE_LEN);
        }
        assert!(t.contains("ZZ1         0   0   00  0   0   0   0   00  0   0   0   0"));
    }

    #[test]
    fn long_values_are_truncated_not_overflowed() {
        let mut g = GedFile::default();
        let mut r = GedRow::new("a-very-long-specimen-name");
        r.fol1 = [123.456, 7.0];
        g.rows.push(r);
        for l in g.to_text().split("\r\n").filter(|l| !l.is_empty()) {
            assert_eq!(l.len(), LINE_LEN);
        }
    }
}
