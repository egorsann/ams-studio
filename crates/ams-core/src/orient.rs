//! Per-specimen orientation stored inside each `.ams` record (written by Anisoft's Edit Data).
//!
//! Layout decoded by saving made-up GED values through the original (every field distinct):
//!
//! | offset | content |
//! |---|---|
//! | 82, 84, 86, 88 | int16 OP1, OP2, OP3, OP4 |
//! | 90 | int16 `-1` when the record carries orientation data |
//! | 92, 96 | float32 sampling azimuth alpha, plunge phi |
//! | 100 | 4 bytes: foliation #1 code, space, foliation #2 code, space |
//! | 108, 112 | float32 azimuth of foliation #1, #2 |
//! | 124, 128 | float32 dip of foliation #1, #2 |
//! | 140 | 4 bytes: lineation #1 code, space, lineation #2 code, space |
//! | 148, 152 | float32 trend of lineation #1, #2 |
//! | 164, 168 | float32 plunge of lineation #1, #2 |
//!
//! The geographic rotation was verified on 12 specimens for three OP sets (9,90,3,90), (6,0,6,0), (12,0,9,0)
//! to the 0.1 degree the original prints. Paleo and tectonic coordinates are NOT decoded here.

use crate::tensor::{mat_mul, Mat3};

pub const OFF_OP: usize = 82;
pub const OFF_FLAG: usize = 90;
pub const OFF_ALPHA: usize = 92;
pub const OFF_PHI: usize = 96;
pub const OFF_FOL_CODE: usize = 100;
pub const OFF_FOL_AZI: usize = 108;
pub const OFF_FOL_DIP: usize = 124;
pub const OFF_LIN_CODE: usize = 140;
pub const OFF_LIN_TREND: usize = 148;
pub const OFF_LIN_PLUNGE: usize = 164;

#[derive(Debug, Clone, PartialEq)]
pub struct RecOrient {
    pub op: [i16; 4],
    pub alpha: f64,
    pub phi: f64,
    pub fol_code: [char; 2],
    pub fol: [(f64, f64); 2],
    pub lin_code: [char; 2],
    pub lin: [(f64, f64); 2],
}

impl Default for RecOrient {
    fn default() -> Self {
        RecOrient { op: [6, 0, 6, 0], alpha: 0.0, phi: 0.0, fol_code: [' '; 2], fol: [(0.0, 0.0); 2], lin_code: [' '; 2], lin: [(0.0, 0.0); 2] }
    }
}

fn f32_at(b: &[u8], o: usize) -> f64 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64
}

fn code_at(b: &[u8], o: usize) -> char {
    match b[o] {
        c if c.is_ascii_graphic() => c as char,
        _ => ' ',
    }
}

/// The orientation of a record, `None` when the record carries none (flag at 90 is not -1).
pub fn read(rec: &[u8]) -> Option<RecOrient> {
    if i16::from_le_bytes([rec[OFF_FLAG], rec[OFF_FLAG + 1]]) != -1 {
        return None;
    }
    let i16_at = |o: usize| i16::from_le_bytes([rec[o], rec[o + 1]]);
    Some(RecOrient {
        op: [i16_at(OFF_OP), i16_at(OFF_OP + 2), i16_at(OFF_OP + 4), i16_at(OFF_OP + 6)],
        alpha: f32_at(rec, OFF_ALPHA),
        phi: f32_at(rec, OFF_PHI),
        fol_code: [code_at(rec, OFF_FOL_CODE), code_at(rec, OFF_FOL_CODE + 2)],
        fol: [(f32_at(rec, OFF_FOL_AZI), f32_at(rec, OFF_FOL_DIP)), (f32_at(rec, OFF_FOL_AZI + 4), f32_at(rec, OFF_FOL_DIP + 4))],
        lin_code: [code_at(rec, OFF_LIN_CODE), code_at(rec, OFF_LIN_CODE + 2)],
        lin: [(f32_at(rec, OFF_LIN_TREND), f32_at(rec, OFF_LIN_PLUNGE)), (f32_at(rec, OFF_LIN_TREND + 4), f32_at(rec, OFF_LIN_PLUNGE + 4))],
    })
}

/// Writes the orientation into a record, or clears the flag for `None` (the other bytes of the record are untouched).
pub fn write(rec: &mut [u8], o: Option<&RecOrient>) {
    let Some(o) = o else {
        rec[OFF_FLAG..OFF_FLAG + 2].copy_from_slice(&0i16.to_le_bytes());
        return;
    };
    for (i, v) in o.op.iter().enumerate() {
        rec[OFF_OP + 2 * i..OFF_OP + 2 * i + 2].copy_from_slice(&v.to_le_bytes());
    }
    rec[OFF_FLAG..OFF_FLAG + 2].copy_from_slice(&(-1i16).to_le_bytes());
    let mut put = |off: usize, v: f64| rec[off..off + 4].copy_from_slice(&(v as f32).to_le_bytes());
    put(OFF_ALPHA, o.alpha);
    put(OFF_PHI, o.phi);
    for k in 0..2 {
        put(OFF_FOL_AZI + 4 * k, o.fol[k].0);
        put(OFF_FOL_DIP + 4 * k, o.fol[k].1);
        put(OFF_LIN_TREND + 4 * k, o.lin[k].0);
        put(OFF_LIN_PLUNGE + 4 * k, o.lin[k].1);
    }
    for (off, codes) in [(OFF_FOL_CODE, o.fol_code), (OFF_LIN_CODE, o.lin_code)] {
        rec[off..off + 4].copy_from_slice(&[codes[0] as u8, b' ', codes[1] as u8, b' ']);
    }
}

fn rz(deg: f64) -> Mat3 {
    // this crate's axes (x east, y north, z down): adds `deg` to the declination
    let (s, c) = deg.to_radians().sin_cos();
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn rx(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

/// Specimen -> geographic rotation of a record:
/// `Rz((6 - OP3) 30) Rz(alpha) Rx(g) Rz((OP1 - 6) 30)`, `g = phi` for OP2 = 0 and `90 - phi` for OP2 = 90
/// (OP1, OP3 in {3, 6, 9, 12}, the clock positions of the sampling arrow). OP4 does not matter here.
pub fn geo_rotation(o: &RecOrient) -> Mat3 {
    let g = if o.op[1] == 0 { o.phi } else { 90.0 - o.phi };
    let m = mat_mul(&rz(f64::from(6 - o.op[2]) * 30.0), &rz(o.alpha));
    mat_mul(&mat_mul(&m, &rx(g)), &rz(f64::from(o.op[0] - 6) * 30.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::{dec_inc, from_dec_inc, mat_vec};

    fn sample() -> RecOrient {
        RecOrient {
            op: [9, 90, 3, 90],
            alpha: 41.0,
            phi: 31.0,
            fol_code: ['7', '5'],
            fol: [(61.0, 21.0), (151.0, 44.0)],
            lin_code: ['1', '2'],
            lin: [(101.0, 33.0), (201.0, 55.0)],
        }
    }

    #[test]
    fn record_layout_round_trips() {
        let mut rec = vec![0u8; 640];
        assert!(read(&rec).is_none());
        write(&mut rec, Some(&sample()));
        assert_eq!(&rec[100..104], b"7 5 ");
        assert_eq!(&rec[140..144], b"1 2 ");
        assert_eq!(f32::from_le_bytes([rec[92], rec[93], rec[94], rec[95]]), 41.0);
        assert_eq!(read(&rec), Some(sample()));
        write(&mut rec, None);
        assert!(read(&rec).is_none());
    }

    /// Anisoft 6 printed these geographic directions for made-up spec axes (dec, inc) of one specimen.
    #[test]
    fn geographic_rule_matches_anisoft_6() {
        let spec = [(22.9, 0.0), (112.9, 30.1), (292.9, 59.9)];
        let cases: [(RecOrient, [(f64, f64); 3]); 3] = [
            (sample(), [(53.3, 19.5), (152.8, 25.1), (289.8, 57.3)]),
            (RecOrient { op: [6, 0, 6, 0], ..sample() }, [(67.2, 28.3), (165.5, 14.9), (279.9, 57.4)]),
            (RecOrient { op: [12, 0, 9, 0], ..sample() }, [(337.2, 28.3), (223.2, 37.1), (94.0, 39.9)]),
        ];
        for (o, want) in cases {
            let r = geo_rotation(&o);
            for (s, w) in spec.iter().zip(want.iter()) {
                let got = dec_inc(mat_vec(&r, from_dec_inc(s.0, s.1)));
                let dd = ((got.0 - w.0 + 540.0) % 360.0 - 180.0).abs();
                assert!(dd < 0.3 && (got.1 - w.1).abs() < 0.3, "{o:?}: {s:?} -> {got:?}, want {w:?}");
            }
        }
    }
}
