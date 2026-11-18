//! In-phase and out-of-phase data that were exported as two files (Anisoft exports of one specimen set, or Anisoft 4.2's
//! `.ran` pairs `X.ran` + `X_Im.ran`), and how they are joined into full records again.
//!
//! A full `.ams` record holds both phases (in-phase block at byte 180, out-of-phase block at byte 324). A split export
//! holds one phase per file; which slot the data sits in is not the same everywhere (a `.ran` converted to records
//! always puts its numbers in the in-phase slot), so the out-of-phase source is the out-of-phase block when that
//! carries numbers and the in-phase block otherwise.

use crate::ams::{OFF_KIM, OFF_KRE, OFF_NAME, RECORD_LEN};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    InPhase,
    OutOfPhase,
}

/// Name words that say which phase a file or record holds (compared in lower case).
const IN_WORDS: &[&[&str]] = &[&["in", "phase"], &["inphase"], &["ip"], &["real"], &["in"]];
const OUT_WORDS: &[&[&str]] = &[&["out", "of", "phase"], &["out", "phase"], &["outphase"], &["outofphase"], &["oop"], &["op"], &["im"], &["imag"], &["imaginary"], &["quad"], &["quadrature"], &["out"]];

fn tokens(s: &str) -> Vec<(String, String)> {
    // (separator before, word)
    let mut out: Vec<(String, String)> = vec![];
    let (mut sep, mut word) = (String::new(), String::new());
    for c in s.chars() {
        if matches!(c, '_' | '-' | ' ' | '.') {
            if !word.is_empty() {
                out.push((std::mem::take(&mut sep), std::mem::take(&mut word)));
            }
            sep.push(c);
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push((sep, word));
    }
    out
}

/// A name without its phase word, and the phase the word named: `JE19ET101_Im` -> (`JE19ET101`, out of phase),
/// `ER26PB002_use_oop` -> (`ER26PB002_use`, out of phase), `X_in_phase` -> (`X`, in phase). A name without such a word
/// comes back unchanged with `None`. Only a whole word counts (`Impact` is not `Im`), and never the first word of a
/// name, which is the specimen.
pub fn split_phase_word(name: &str) -> (String, Option<Role>) {
    let toks = tokens(name.trim());
    let low: Vec<String> = toks.iter().map(|t| t.1.to_lowercase()).collect();
    for (words, role) in IN_WORDS.iter().map(|w| (*w, Role::InPhase)).chain(OUT_WORDS.iter().map(|w| (*w, Role::OutOfPhase))) {
        let n = words.len();
        if toks.len() <= n {
            continue;
        }
        // later words first: the tag is normally at the end
        for start in (1..=toks.len() - n).rev() {
            if low[start..start + n].iter().map(|s| s.as_str()).eq(words.iter().copied()) {
                // the separator in front of a removed word goes with it; every kept word keeps its own
                let mut keep = String::new();
                for (i, (sep, w)) in toks.iter().enumerate() {
                    if i >= start && i < start + n {
                        continue;
                    }
                    keep.push_str(sep);
                    keep.push_str(w);
                }
                return (keep, Some(role));
            }
        }
    }
    (name.trim().to_string(), None)
}

/// The record name of a split export without the phase word (`A2_Im` -> `A2`).
pub fn base_name(name: &str) -> String {
    split_phase_word(name).0
}

/// One pair of files (or a file that found no partner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub key: String,
    pub ip: Option<usize>,
    pub op: Option<usize>,
}

impl Pair {
    pub fn complete(&self) -> bool {
        self.ip.is_some() && self.op.is_some()
    }
}

/// Pairs files by their names. `stems` are file names without the extension. A file is the out-of-phase half when its
/// name carries an out-of-phase word and another file has the same name without it (or with an in-phase word).
/// Files that cannot be paired come back as pairs with one side empty.
pub fn pair_files(stems: &[String]) -> Vec<Pair> {
    let info: Vec<(String, Option<Role>)> = stems.iter().map(|s| split_phase_word(s)).collect();
    let mut groups: Vec<(String, Vec<usize>)> = vec![];
    for (i, (k, _)) in info.iter().enumerate() {
        let key = k.to_lowercase();
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    let mut out = vec![];
    for (_, idx) in groups {
        let ops: Vec<usize> = idx.iter().copied().filter(|&i| info[i].1 == Some(Role::OutOfPhase)).collect();
        let ips: Vec<usize> = idx.iter().copied().filter(|&i| info[i].1 != Some(Role::OutOfPhase)).collect();
        let key = info[idx[0]].0.clone();
        if ops.len() == 1 && ips.len() == 1 {
            out.push(Pair { key, ip: Some(ips[0]), op: Some(ops[0]) });
        } else if ops.len() == ips.len() && ops.len() > 1 {
            // several of each: the n-th in-phase file with the n-th out-of-phase file
            for (a, b) in ips.iter().zip(&ops) {
                out.push(Pair { key: key.clone(), ip: Some(*a), op: Some(*b) });
            }
        } else {
            for &i in &ips {
                out.push(Pair { key: key.clone(), ip: Some(i), op: None });
            }
            for &i in &ops {
                out.push(Pair { key: key.clone(), ip: None, op: Some(i) });
            }
        }
    }
    out
}

fn f32_at(rec: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]])
}

/// Whether the block starting at `base` carries numbers (a bulk susceptibility or any tensor component).
pub fn block_has_data(rec: &[u8], base: usize) -> bool {
    if rec.len() < base + 56 {
        return false;
    }
    let nonzero = |o: usize| {
        let v = f32_at(rec, base + o);
        v != 0.0 && v.is_finite()
    };
    nonzero(0) || (0..6).any(|i| nonzero(32 + 4 * i))
}

/// What the join of two files did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergeReport {
    /// Records that got both phases.
    pub joined: usize,
    /// How many of them took the out-of-phase numbers from the in-phase slot of the second file.
    pub from_first_slot: usize,
    /// In-phase records without a partner (kept, out-of-phase block empty).
    pub ip_only: Vec<String>,
    /// Out-of-phase records without a partner (left out).
    pub op_only: Vec<String>,
    /// The names did not match, so the records were joined in file order (equal counts).
    pub by_position: bool,
}

fn name_key(rec: &[u8]) -> String {
    let raw: String = rec[OFF_NAME..OFF_NAME + 24].iter().take_while(|b| **b != 0).map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect();
    base_name(raw.trim()).to_lowercase()
}

fn name_text(rec: &[u8]) -> String {
    let raw: String = rec[OFF_NAME..OFF_NAME + 24].iter().take_while(|b| **b != 0).map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect();
    raw.trim().to_string()
}

/// Joins an in-phase file and an out-of-phase file (both as 640-byte records) into full records, in the in-phase
/// file's order. Records are matched by name (a phase word such as `_Im` is ignored, case does not matter; equal names
/// are matched in order); when no name matches and both files have the same number of records they are joined in
/// file order.
pub fn merge_phases(ip: &[u8], op: &[u8]) -> Result<(Vec<u8>, MergeReport), String> {
    if ip.is_empty() || ip.len() % RECORD_LEN != 0 {
        return Err("the in-phase file is not a whole number of 640-byte .ams records".into());
    }
    if op.is_empty() || op.len() % RECORD_LEN != 0 {
        return Err("the out-of-phase file is not a whole number of 640-byte .ams records".into());
    }
    let a: Vec<&[u8]> = ip.chunks_exact(RECORD_LEN).collect();
    let b: Vec<&[u8]> = op.chunks_exact(RECORD_LEN).collect();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in b.iter().enumerate() {
        by_name.entry(name_key(r)).or_default().push(i);
    }
    let any_name = a.iter().any(|r| by_name.contains_key(&name_key(r)));
    let by_position = !any_name && a.len() == b.len();
    let mut used = vec![false; b.len()];
    let mut out = Vec::with_capacity(ip.len());
    let mut rep = MergeReport { by_position, ..Default::default() };
    let mut next: HashMap<String, usize> = HashMap::new();
    for (i, r) in a.iter().enumerate() {
        let mut rec = r.to_vec();
        let partner = if by_position {
            Some(i)
        } else {
            let k = name_key(r);
            let n = next.entry(k.clone()).or_insert(0);
            let hit = by_name.get(&k).and_then(|v| v.get(*n)).copied();
            if hit.is_some() {
                *n += 1;
            }
            hit
        };
        match partner {
            Some(j) => {
                used[j] = true;
                let src = b[j];
                let (from, first) = if block_has_data(src, OFF_KIM) { (OFF_KIM, false) } else { (OFF_KRE, true) };
                if block_has_data(src, from) {
                    rec[OFF_KIM..OFF_KIM + 144].copy_from_slice(&src[from..from + 144]);
                    rep.joined += 1;
                    rep.from_first_slot += first as usize;
                } else {
                    rep.ip_only.push(name_text(r));
                }
            }
            None => rep.ip_only.push(name_text(r)),
        }
        // the record name never keeps the phase word
        let (stripped, role) = split_phase_word(&name_text(&rec));
        let clean = if role == Some(Role::OutOfPhase) { stripped } else { name_text(&rec) };
        rec[OFF_NAME..OFF_NAME + 24].fill(0);
        for (k, c) in clean.bytes().take(24).enumerate() {
            rec[OFF_NAME + k] = c;
        }
        out.extend(rec);
    }
    for (j, r) in b.iter().enumerate() {
        if !used[j] {
            rep.op_only.push(name_text(r));
        }
    }
    Ok((out, rep))
}

/// Which of two files holds the out-of-phase data when their names do not say: the out-of-phase bulk susceptibility is
/// far smaller than the in-phase one (about 1000 times). `None` when the two are alike.
pub fn guess_out_of_phase(a: &[u8], b: &[u8]) -> Option<usize> {
    let mean = |x: &[u8]| -> f64 {
        let v: Vec<f64> = x.chunks_exact(RECORD_LEN).flat_map(|r| [f32_at(r, OFF_KRE).abs() as f64, f32_at(r, OFF_KIM).abs() as f64].into_iter().filter(|v| *v > 0.0)).collect();
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let (ma, mb) = (mean(a), mean(b));
    if ma <= 0.0 || mb <= 0.0 {
        return None;
    }
    if ma > mb * 20.0 {
        Some(1)
    } else if mb > ma * 20.0 {
        Some(0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ams::{build_record_with, parse, Phase};
    use crate::tensor::sym;

    fn ph(km: f64, d: f64) -> Phase {
        Phase { km, sd: [0.0; 3], tensor: sym(d, d + 0.1, 3.0 - 2.0 * d - 0.1, 0.01, 0.0, 0.0), ftest: [0.0; 3] }
    }
    fn zero() -> Phase {
        Phase { km: 0.0, sd: [0.0; 3], tensor: [[0.0; 3]; 3], ftest: [0.0; 3] }
    }

    #[test]
    fn phase_words_are_found_as_whole_words() {
        assert_eq!(split_phase_word("JE19ET101_Im"), ("JE19ET101".into(), Some(Role::OutOfPhase)));
        assert_eq!(split_phase_word("ER26PB002_use_oop"), ("ER26PB002_use".into(), Some(Role::OutOfPhase)));
        assert_eq!(split_phase_word("X-in-phase"), ("X".into(), Some(Role::InPhase)));
        assert_eq!(split_phase_word("X out of phase"), ("X".into(), Some(Role::OutOfPhase)));
        assert_eq!(split_phase_word("A2_Im"), ("A2".into(), Some(Role::OutOfPhase)));
        assert_eq!(split_phase_word("ZZ01_Impact"), ("ZZ01_Impact".into(), None));
        assert_eq!(split_phase_word("Im"), ("Im".into(), None), "a lone word is the specimen, not a tag");
        assert_eq!(split_phase_word("ZZ01-A1"), ("ZZ01-A1".into(), None));
        assert_eq!(split_phase_word("SAMPLE_real_final"), ("SAMPLE_final".into(), Some(Role::InPhase)));
    }

    #[test]
    fn files_are_paired_by_name() {
        let s: Vec<String> = ["A_use", "A_use_Im", "B", "B_oop", "C"].iter().map(|s| s.to_string()).collect();
        let p = pair_files(&s);
        assert_eq!(p.len(), 3);
        assert!(p.iter().any(|x| x.key == "A_use" && x.ip == Some(0) && x.op == Some(1)));
        assert!(p.iter().any(|x| x.key == "B" && x.ip == Some(2) && x.op == Some(3)));
        assert!(p.iter().any(|x| x.key == "C" && x.ip == Some(4) && x.op.is_none()));
        // tagged both ways
        let s: Vec<String> = ["S_in", "S_out"].iter().map(|s| s.to_string()).collect();
        assert!(pair_files(&s)[0].complete());
        // two untagged files with one name are not guessed
        let s: Vec<String> = vec!["X".into(), "x".into()];
        assert!(pair_files(&s).iter().all(|p| !p.complete()));
    }

    #[test]
    fn two_files_become_full_records() {
        let mut a = build_record_with("ZZ01-A1", &ph(2e-3, 1.0), &zero(), 400.0, 1220.0, "KLY5");
        a.extend(build_record_with("ZZ01-B1", &ph(3e-3, 0.9), &zero(), 400.0, 1220.0, "KLY5"));
        // the out-of-phase file keeps its numbers in the first slot, names carry _Im, order differs
        let mut b = build_record_with("ZZ01-B1_Im", &ph(-4e-6, 1.1), &zero(), 400.0, 1220.0, "KLY5");
        b.extend(build_record_with("ZZ01-A1_Im", &ph(-1e-6, 1.2), &zero(), 400.0, 1220.0, "KLY5"));
        let (m, rep) = merge_phases(&a, &b).unwrap();
        assert_eq!((rep.joined, rep.from_first_slot, rep.by_position), (2, 2, false));
        let s = parse(&m).unwrap();
        assert_eq!(s[0].name, "ZZ01-A1");
        assert!((s[0].inphase.km - 2e-3).abs() < 1e-9 && (s[0].outphase.km + 1e-6).abs() < 1e-12);
        assert!((s[1].outphase.km + 4e-6).abs() < 1e-12);
        assert!((s[1].outphase.tensor[0][0] - 1.1).abs() < 1e-6);
    }

    #[test]
    fn a_full_second_file_gives_its_out_of_phase_block_and_gaps_are_reported() {
        let mut a = build_record_with("S1", &ph(1e-3, 1.0), &zero(), 0.0, 0.0, "");
        a.extend(build_record_with("S2", &ph(1e-3, 1.0), &zero(), 0.0, 0.0, ""));
        let b = build_record_with("S1", &ph(5e-3, 0.7), &ph(-2e-6, 1.3), 0.0, 0.0, "");
        let (m, rep) = merge_phases(&a, &b).unwrap();
        assert_eq!((rep.joined, rep.from_first_slot), (1, 0));
        assert_eq!(rep.ip_only, ["S2"]);
        let s = parse(&m).unwrap();
        assert!((s[0].outphase.km + 2e-6).abs() < 1e-12 && (s[0].inphase.km - 1e-3).abs() < 1e-9);
        assert!(s[1].outphase.km == 0.0);
        assert!(merge_phases(&a[..100], &b).is_err());
    }

    #[test]
    fn unmatched_names_with_equal_counts_join_by_position() {
        let a = build_record_with("one", &ph(1e-3, 1.0), &zero(), 0.0, 0.0, "");
        let b = build_record_with("uno", &ph(-1e-6, 1.0), &zero(), 0.0, 0.0, "");
        let (m, rep) = merge_phases(&a, &b).unwrap();
        assert!(rep.by_position && rep.joined == 1);
        assert!((parse(&m).unwrap()[0].outphase.km + 1e-6).abs() < 1e-12);
    }

    #[test]
    fn the_smaller_file_is_the_out_of_phase_one() {
        let a = build_record_with("S", &ph(2e-3, 1.0), &zero(), 0.0, 0.0, "");
        let b = build_record_with("S", &ph(-3e-6, 1.0), &zero(), 0.0, 0.0, "");
        assert_eq!(guess_out_of_phase(&a, &b), Some(1));
        assert_eq!(guess_out_of_phase(&b, &a), Some(0));
        assert_eq!(guess_out_of_phase(&a, &a), None);
    }
}
