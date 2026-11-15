//! Recombining split `.ams` files: the records of several files are joined into one list, in a suggested order.
//!
//! The suggested order groups records by sample and then runs through the sub-specimens: with the IDs
//! `ZZ01-A1 ZZ01-A2 ZZ01-B1 ZZ01-B2 ZZ01-C1` the default gives `A1 B1 C1 A2 B2` (number first, then letter);
//! the other choice gives `A1 A2 B1 B2 C1`. The sample part is compared the way a person reads it (`ZZ2` before
//! `ZZ10`). Records whose name does not end in letters + digits after a `-`, `_` or space keep their place by
//! their name and by where they were in the source files; the sort is stable, so equal keys keep the order of the
//! files as listed and of the records inside each file.

use std::cmp::Ordering;

/// What the planner needs to know about one record.
#[derive(Debug, Clone, PartialEq)]
pub struct RecInfo {
    pub name: String,
    /// The record's bytes (to tell exact duplicates from different measurements with the same name).
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Within {
    /// A1 B1 C1 A2 B2 C2
    NumberThenLetter,
    /// A1 A2 B1 B2 C1 C2
    LetterThenNumber,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Order by specimen ID; off = the files as listed, one after the other (append / prepend by file order).
    pub order: bool,
    pub within: Within,
    /// Leave out a record that is byte for byte the same as an earlier one.
    pub skip_duplicates: bool,
    /// Keep only the last record of each name (the one from the file listed last).
    pub one_per_name: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { order: true, within: Within::NumberThenLetter, skip_duplicates: true, one_per_name: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    Duplicate,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planned {
    pub file: usize,
    pub idx: usize,
    pub skip: Option<Skip>,
}

/// `(sample part, letters, number)` of a name like `ZZ01-A1`.
pub fn split_id(name: &str) -> (String, Option<(String, u64)>) {
    let name = name.trim();
    // the sub-specimen part is letters + number (A1) or number + letters (1A)
    let tail_parts = |tail: &str| -> Option<(String, u64)> {
        let letters: String = tail.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let digits = &tail[letters.len()..];
        if !letters.is_empty() && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return Some((letters.to_lowercase(), digits.parse::<u64>().ok()?));
        }
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        let letters = &tail[digits.len()..];
        if !digits.is_empty() && !letters.is_empty() && letters.chars().all(|c| c.is_ascii_alphabetic()) {
            return Some((letters.to_lowercase(), digits.parse::<u64>().ok()?));
        }
        None
    };
    // after the last separator: ZZ01-A1, ZZ01_c12, ZZ01 2B
    if let Some(p) = name.rfind(|c: char| matches!(c, '-' | '_' | ' ')) {
        if let Some(t) = tail_parts(&name[p + 1..]) {
            return (name[..p].to_string(), Some(t));
        }
    }
    // no separator: trailing letters + number that follow a digit of the sample name (DC26AB002A1 -> DC26AB002 | A1)
    let digits_at_end = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits_at_end > 0 {
        let head = &name[..name.len() - digits_at_end];
        let letters_at_end = head.len() - head.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len();
        if letters_at_end > 0 {
            let sample = &head[..head.len() - letters_at_end];
            if sample.ends_with(|c: char| c.is_ascii_digit()) {
                if let Some(t) = tail_parts(&name[sample.len()..]) {
                    return (sample.to_string(), Some(t));
                }
            }
        }
    }
    (name.to_string(), None)
}

fn chunks(s: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = vec![];
    for c in s.to_lowercase().chars() {
        let d = c.is_ascii_digit();
        match out.last_mut() {
            Some((ld, t)) if *ld == d => t.push(c),
            _ => out.push((d, c.to_string())),
        }
    }
    out
}

/// Natural comparison: runs of digits compare as numbers, the rest as lower-case text.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(&cb) {
        let o = if x.0 && y.0 {
            let (nx, ny) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            nx.len().cmp(&ny.len()).then_with(|| nx.cmp(ny))
        } else {
            x.1.cmp(&y.1)
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

fn order_cmp(a: &str, b: &str, within: Within) -> Ordering {
    let ((pa, ta), (pb, tb)) = (split_id(a), split_id(b));
    natural_cmp(&pa, &pb).then_with(|| match (ta, tb) {
        (Some((la, na)), Some((lb, nb))) => match within {
            Within::NumberThenLetter => na.cmp(&nb).then_with(|| la.cmp(&lb)),
            Within::LetterThenNumber => la.cmp(&lb).then_with(|| na.cmp(&nb)),
        },
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    })
}

/// Whether one file's records are already in the suggested order.
pub fn is_sorted(records: &[RecInfo], within: Within) -> bool {
    records.windows(2).all(|w| order_cmp(&w[0].name, &w[1].name, within) != Ordering::Greater)
}

/// The combined list for the files in the order given.
pub fn plan(files: &[Vec<RecInfo>], o: &Options) -> Vec<Planned> {
    let mut all: Vec<Planned> = files.iter().enumerate().flat_map(|(f, r)| (0..r.len()).map(move |i| Planned { file: f, idx: i, skip: None })).collect();
    if o.order {
        all.sort_by(|a, b| order_cmp(&files[a.file][a.idx].name, &files[b.file][b.idx].name, o.within)); // stable: ties keep file and record order
    }
    let rec = |p: &Planned| &files[p.file][p.idx];
    if o.one_per_name {
        // the record of the file listed last (and the last inside it) wins
        for i in 0..all.len() {
            let name = rec(&all[i]).name.trim().to_lowercase();
            let later = all.iter().any(|q| rec(q).name.trim().to_lowercase() == name && (q.file, q.idx) > (all[i].file, all[i].idx));
            if later {
                all[i].skip = Some(Skip::Superseded);
            }
        }
    }
    if o.skip_duplicates {
        for i in 0..all.len() {
            if all[i].skip.is_none() {
                let dup = all[..i].iter().any(|q| q.skip.is_none() && rec(q).bytes == rec(&all[i]).bytes);
                if dup {
                    all[i].skip = Some(Skip::Duplicate);
                }
            }
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, tag: u8) -> RecInfo {
        RecInfo { name: name.into(), bytes: vec![tag; 4] }
    }

    fn names(files: &[Vec<RecInfo>], p: &[Planned]) -> Vec<String> {
        p.iter().map(|x| files[x.file][x.idx].name.clone()).collect()
    }

    #[test]
    fn ids_are_split_into_sample_letter_number() {
        assert_eq!(split_id("ZZ01-A1"), ("ZZ01".into(), Some(("a".into(), 1))));
        assert_eq!(split_id("DC26AB002_c12"), ("DC26AB002".into(), Some(("c".into(), 12))));
        assert_eq!(split_id("ZZ01").1, None);
        assert_eq!(split_id("ZZ01-7").1, None);
        // number + letter, and names without a separator
        assert_eq!(split_id("ZZ01-2B"), ("ZZ01".into(), Some(("b".into(), 2))));
        assert_eq!(split_id("DC26AB002A1"), ("DC26AB002".into(), Some(("a".into(), 1))));
        assert_eq!(split_id("SPX12C3"), ("SPX12".into(), Some(("c".into(), 3))));
        assert_eq!(split_id("ZZ01").1, None);
        assert_eq!(split_id("AB12").1, None);
    }

    #[test]
    fn the_choice_between_the_two_orders_changes_number_letter_names_too() {
        let f = vec![vec![r("ZZ01-1A", 1), r("ZZ01-2A", 2), r("ZZ01-1B", 3), r("ZZ01-2B", 4)]];
        let a = plan(&f, &Options::default());
        let b = plan(&f, &Options { within: Within::LetterThenNumber, ..Default::default() });
        assert_eq!(names(&f, &a), ["ZZ01-1A", "ZZ01-1B", "ZZ01-2A", "ZZ01-2B"]);
        assert_eq!(names(&f, &b), ["ZZ01-1A", "ZZ01-2A", "ZZ01-1B", "ZZ01-2B"]);
        let g = vec![vec![r("DC26AB002A2", 1), r("DC26AB002B1", 2), r("DC26AB002A1", 3)]];
        assert_ne!(names(&g, &plan(&g, &Options::default())), names(&g, &plan(&g, &Options { within: Within::LetterThenNumber, ..Default::default() })));
    }

    #[test]
    fn natural_order_of_samples() {
        assert_eq!(natural_cmp("ZZ2", "ZZ10"), Ordering::Less);
        assert_eq!(natural_cmp("zz02", "ZZ2"), Ordering::Equal.then(Ordering::Equal));
        assert_eq!(natural_cmp("a", "A"), Ordering::Equal);
    }

    #[test]
    fn suggested_order_runs_through_the_letters_first() {
        let f = vec![vec![r("ZZ01-A2", 1), r("ZZ01-B1", 2)], vec![r("ZZ01-C1", 3), r("ZZ01-A1", 4), r("ZZ01-B2", 5)]];
        let p = plan(&f, &Options::default());
        assert_eq!(names(&f, &p), ["ZZ01-A1", "ZZ01-B1", "ZZ01-C1", "ZZ01-A2", "ZZ01-B2"]);
        let p = plan(&f, &Options { within: Within::LetterThenNumber, ..Default::default() });
        assert_eq!(names(&f, &p), ["ZZ01-A1", "ZZ01-A2", "ZZ01-B1", "ZZ01-B2", "ZZ01-C1"]);
    }

    #[test]
    fn without_ordering_the_files_follow_each_other() {
        let f = vec![vec![r("ZZ01-B1", 1)], vec![r("ZZ01-A1", 2)]];
        let p = plan(&f, &Options { order: false, ..Default::default() });
        assert_eq!(names(&f, &p), ["ZZ01-B1", "ZZ01-A1"]);
        // listing the other file first prepends it
        let g = vec![f[1].clone(), f[0].clone()];
        assert_eq!(names(&g, &plan(&g, &Options { order: false, ..Default::default() })), ["ZZ01-A1", "ZZ01-B1"]);
    }

    #[test]
    fn samples_stay_together_and_unreadable_names_keep_their_files_order() {
        let f = vec![vec![r("ZZ10-A1", 1), r("ZZ2-B1", 2), r("odd one", 3), r("ZZ2-A1", 4), r("another", 5)]];
        let p = plan(&f, &Options::default());
        let n = names(&f, &p);
        assert_eq!(n[..4].to_vec(), ["another", "odd one", "ZZ2-A1", "ZZ2-B1"]);
        assert_eq!(n[4], "ZZ10-A1");
        assert!(is_sorted(&[r("ZZ01-A1", 0), r("ZZ01-B1", 0), r("ZZ01-A2", 0)], Within::NumberThenLetter));
        assert!(!is_sorted(&[r("ZZ01-A2", 0), r("ZZ01-A1", 0)], Within::NumberThenLetter));
    }

    #[test]
    fn duplicates_and_superseded_records_are_marked() {
        let f = vec![vec![r("ZZ01-A1", 1), r("ZZ01-B1", 2)], vec![r("ZZ01-A1", 1), r("ZZ01-B1", 9)]];
        let p = plan(&f, &Options::default());
        let skipped: Vec<(usize, usize)> = p.iter().filter(|x| x.skip == Some(Skip::Duplicate)).map(|x| (x.file, x.idx)).collect();
        assert_eq!(skipped, [(1, 0)], "only the byte-identical record is a duplicate");
        let p = plan(&f, &Options { one_per_name: true, ..Default::default() });
        assert!(p.iter().filter(|x| x.skip == Some(Skip::Superseded)).all(|x| x.file == 0));
        assert_eq!(p.iter().filter(|x| x.skip.is_none()).count(), 2);
    }
}
