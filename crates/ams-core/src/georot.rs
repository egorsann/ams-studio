//! The reorientation of a record, stored inside the record.
//!
//! An `_geo` file is a byte-for-byte copy of its source file, so the specimen coordinates stay exactly as measured,
//! plus this block in every reoriented record. The geographic view (F6) is computed on the fly from it.
//!
//! Anisoft's own orientation fields (`orient.rs`: sampling azimuth and plunge, arrow clock positions OP1 / OP3 in
//! steps of 30 degrees) cannot hold a continuous offset such as the 356, 352 or 45 degree orientation-line offsets of
//! the drill cores, so the exact reorientation lives in the trailing padding of the 640-byte record. Those 24 bytes
//! (offset 616 to 639) were zero in all 1432 records of the project's own files (Safyr7 output and Anisoft edits).
//!
//! | offset | size | content |
//! |---|---|---|
//! | 616 | 4 | the text `AMSG` |
//! | 620 | 1 | version, 1 |
//! | 621 | 1 | method: 1 drill core, 2 Owens |
//! | 622 | 1 | drill core: face that holds the arrow (index in `Face::ALL`); Owens: 0 |
//! | 623 | 1 | drill core: arrow direction (index in `Arrow::ALL`); Owens: 0 |
//! | 624 | 16 | four float32: drill core `dip direction, plunge, offset, 0`; Owens `FS, FD, DS, DD` |

use crate::coords::owens_rotation;
use crate::core::{core_rotation, Arrow, CoreSetup, Face};
use crate::tensor::Mat3;

pub const OFF_GEOROT: usize = 616;
const MAGIC: &[u8; 4] = b"AMSG";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeoRot {
    /// A cube of a drill core (see `core.rs`).
    Core(CoreSetup),
    /// Owens' method: strike and dip of a plane in the field and on the drilling table.
    Owens { fs: f64, fd: f64, ds: f64, dd: f64 },
}

impl GeoRot {
    /// Specimen to geographic rotation in this crate's axes (`R T R^T`, `R v`).
    pub fn matrix(&self) -> Mat3 {
        match self {
            GeoRot::Core(s) => core_rotation(s),
            GeoRot::Owens { fs, fd, ds, dd } => owens_rotation(*fs, *fd, *ds, *dd),
        }
    }

    /// A short description for messages and tooltips.
    pub fn describe(&self) -> String {
        match self {
            GeoRot::Core(s) => format!("drill core: dip direction {:.1}, plunge {:.1}, offset {:.1}", s.dip_dir, s.plunge, s.offset),
            GeoRot::Owens { fs, fd, ds, dd } => format!("Owens: FS {fs:.1}, FD {fd:.1}, DS {ds:.1}, DD {dd:.1}"),
        }
    }
}

fn f32_at(b: &[u8], o: usize) -> f64 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64
}

/// The reorientation stored in a record, `None` when it has none (or the block is not one of ours).
pub fn read(rec: &[u8]) -> Option<GeoRot> {
    if rec.len() < OFF_GEOROT + 24 || &rec[OFF_GEOROT..OFF_GEOROT + 4] != MAGIC || rec[OFF_GEOROT + 4] != 1 {
        return None;
    }
    let p: Vec<f64> = (0..4).map(|k| f32_at(rec, OFF_GEOROT + 8 + 4 * k)).collect();
    if p.iter().any(|v| !v.is_finite()) {
        return None;
    }
    match rec[OFF_GEOROT + 5] {
        1 => Some(GeoRot::Core(CoreSetup {
            dip_dir: p[0],
            plunge: p[1],
            offset: p[2],
            face: *Face::ALL.get(rec[OFF_GEOROT + 6] as usize)?,
            arrow: *Arrow::ALL.get(rec[OFF_GEOROT + 7] as usize)?,
        })),
        2 => Some(GeoRot::Owens { fs: p[0], fd: p[1], ds: p[2], dd: p[3] }),
        _ => None,
    }
}

/// Writes the block into a record, or clears it for `None`; nothing else of the record is touched.
pub fn write(rec: &mut [u8], g: Option<&GeoRot>) {
    let block = &mut rec[OFF_GEOROT..OFF_GEOROT + 24];
    block.fill(0);
    let Some(g) = g else { return };
    block[..4].copy_from_slice(MAGIC);
    block[4] = 1;
    let (method, face, arrow, p) = match g {
        GeoRot::Core(s) => (1u8, Face::ALL.iter().position(|f| *f == s.face).unwrap_or(0) as u8, Arrow::ALL.iter().position(|a| *a == s.arrow).unwrap_or(0) as u8, [s.dip_dir, s.plunge, s.offset, 0.0]),
        GeoRot::Owens { fs, fd, ds, dd } => (2u8, 0, 0, [*fs, *fd, *ds, *dd]),
    };
    block[5] = method;
    block[6] = face;
    block[7] = arrow;
    for (k, v) in p.iter().enumerate() {
        block[8 + 4 * k..12 + 4 * k].copy_from_slice(&(*v as f32).to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_round_trips_and_touches_nothing_else() {
        let mut rec = vec![7u8; 640];
        rec[OFF_GEOROT..].fill(0);
        let before = rec.clone();
        assert!(read(&rec).is_none());
        let core = GeoRot::Core(CoreSetup { dip_dir: 316.135, plunge: 67.0, offset: 356.0, face: Face::Left, arrow: Arrow::Down });
        write(&mut rec, Some(&core));
        assert_eq!(&rec[..OFF_GEOROT], &before[..OFF_GEOROT]);
        let back = read(&rec).unwrap();
        match (back, core) {
            (GeoRot::Core(a), GeoRot::Core(b)) => {
                assert!((a.dip_dir - b.dip_dir).abs() < 1e-3 && (a.offset - b.offset).abs() < 1e-3 && a.face == b.face && a.arrow == b.arrow);
            }
            _ => panic!("method lost"),
        }
        let owens = GeoRot::Owens { fs: 30.0, fd: 20.0, ds: 45.0, dd: 10.0 };
        write(&mut rec, Some(&owens));
        assert_eq!(read(&rec), Some(owens));
        write(&mut rec, None);
        assert_eq!(rec, before);
    }

    #[test]
    fn a_rotation_matrix_comes_back_from_the_stored_values() {
        let s = CoreSetup { dip_dir: 305.0, plunge: 69.0, offset: 45.0, ..Default::default() };
        let mut rec = vec![0u8; 640];
        write(&mut rec, Some(&GeoRot::Core(s)));
        let a = read(&rec).unwrap().matrix();
        let b = core_rotation(&s);
        assert!((0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < 1e-6)));
    }
}
