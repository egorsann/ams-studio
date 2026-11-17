//! Reader for AGICO Safyr7 `.ams` files.
//!
//! Layout (see `docs/FORMATS.md`): fixed 640-byte
//! records, no header. Per record:
//! * name at byte 0 (space/NUL padded), `field` A/m at 28, frequency Hz at 40,
//!   instrument text at 44, program text at 52, reference volume (10.0 cm3) at 68;
//! * two identical 144-byte blocks: in-phase ("Kre") at 180, out-of-phase
//!   ("KIm") at 324. Inside a block: `Km` +0, `K1..3_SD` +20, raw tensor +32
//!   in the order Tyy, Txx, Tzz, Txy, Txz, Tyz, F-tests +128.
//!
//! K1-K3 stored at block +8 are NOT trusted: they are recomputed from the tensor.

use crate::tensor::{dec_inc, eig_sym, sym, Mat3, Shape, Vec3};

pub const RECORD_LEN: usize = 640;
pub const OFF_NAME: usize = 0;
pub const NAME_LEN: usize = 24;
pub const OFF_FIELD: usize = 28;
pub const OFF_FREQ: usize = 40;
pub const OFF_INSTRUMENT: usize = 44;
pub const OFF_PROGRAM: usize = 52;
pub const OFF_VOLUME: usize = 68;
pub const OFF_KRE: usize = 180;
pub const OFF_KIM: usize = 324;

#[derive(Debug)]
pub enum AmsError {
    /// File size is not a multiple of 640 bytes.
    BadSize(usize),
    Empty,
}

impl std::fmt::Display for AmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmsError::BadSize(n) => write!(f, "not an .ams file: {n} bytes is not a multiple of {RECORD_LEN}"),
            AmsError::Empty => write!(f, "empty file"),
        }
    }
}
impl std::error::Error for AmsError {}

/// One measurement component (in-phase or out-of-phase) of a specimen.
#[derive(Debug, Clone)]
pub struct Phase {
    /// Bulk susceptibility as stored (volume correction is NOT applied).
    pub km: f64,
    /// K1/K2/K3 standard deviations as stored (normalised units).
    pub sd: [f64; 3],
    /// Raw tensor in specimen coordinates.
    pub tensor: Mat3,
    pub ftest: [f64; 3],
}

#[derive(Debug, Clone)]
pub struct Specimen {
    pub name: String,
    pub field_am: f64,
    pub freq_hz: f64,
    pub instrument: String,
    pub volume_cm3: f64,
    /// Orientation stored in the record (written by Edit Data), if any.
    pub orient: Option<crate::orient::RecOrient>,
    /// Reorientation stored by AMS Studio (see `georot.rs`), if any: the geographic view is computed from it.
    pub georot: Option<crate::georot::GeoRot>,
    pub inphase: Phase,
    pub outphase: Phase,
}

/// Principal axes of a tensor: eigenvalues are also returned normalised to a mean of 1
/// (this is what Anisoft prints as K1/K2/K3).
#[derive(Debug, Clone)]
pub struct Axes {
    pub k_raw: [f64; 3],
    pub k_norm: [f64; 3],
    pub vecs: [Vec3; 3],
    pub dec_inc: [(f64, f64); 3],
    pub shape: Option<Shape>,
}

impl Axes {
    pub fn from_tensor(t: &Mat3) -> Axes {
        let (k_raw, vecs) = eig_sym(t);
        let mean = (k_raw[0] + k_raw[1] + k_raw[2]) / 3.0;
        let k_norm = if mean != 0.0 {
            [k_raw[0] / mean, k_raw[1] / mean, k_raw[2] / mean]
        } else {
            k_raw
        };
        Axes {
            k_raw,
            k_norm,
            vecs,
            dec_inc: [dec_inc(vecs[0]), dec_inc(vecs[1]), dec_inc(vecs[2])],
            shape: Shape::from_k(k_norm),
        }
    }
}

impl Phase {
    pub fn axes(&self) -> Axes {
        Axes::from_tensor(&self.tensor)
    }
}

fn f32_at(b: &[u8], off: usize) -> f64 {
    f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as f64
}

fn text_at(b: &[u8], off: usize, max: usize) -> String {
    let raw = &b[off..off + max];
    let end = raw.iter().position(|&c| c == 0).unwrap_or(max);
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

fn parse_phase(rec: &[u8], base: usize) -> Phase {
    let km = f32_at(rec, base);
    let sd = [f32_at(rec, base + 20), f32_at(rec, base + 24), f32_at(rec, base + 28)];
    // stored order: Tyy, Txx, Tzz, Txy, Txz, Tyz
    let (tyy, txx, tzz) = (f32_at(rec, base + 32), f32_at(rec, base + 36), f32_at(rec, base + 40));
    let (txy, txz, tyz) = (f32_at(rec, base + 44), f32_at(rec, base + 48), f32_at(rec, base + 52));
    Phase {
        km,
        sd,
        tensor: sym(txx, tyy, tzz, txy, txz, tyz),
        ftest: [f32_at(rec, base + 128), f32_at(rec, base + 132), f32_at(rec, base + 136)],
    }
}

pub fn parse_phase_pub(rec: &[u8], base: usize) -> Phase {
    parse_phase(rec, base)
}

/// Overwrite Km and the six tensor floats of the block starting at `base`.
pub fn put_phase(rec: &mut [u8], base: usize, km: f64, t: &Mat3) {
    let mut put = |off: usize, v: f64| rec[base + off..base + off + 4].copy_from_slice(&(v as f32).to_le_bytes());
    put(0, km);
    for (i, v) in [t[1][1], t[0][0], t[2][2], t[0][1], t[0][2], t[1][2]].into_iter().enumerate() {
        put(32 + 4 * i, v);
    }
}

/// Rotates both tensors of one 640-byte record by `r` (`R T R^T`) and writes them back; Km, standard deviations and
/// F-tests are unchanged by a rotation.
pub fn rotate_record(rec: &mut [u8], r: &Mat3) {
    for base in [OFF_KRE, OFF_KIM] {
        let ph = parse_phase(rec, base);
        put_phase(rec, base, ph.km, &crate::tensor::rotate_tensor(&ph.tensor, r));
    }
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Specimen>, AmsError> {
    if bytes.is_empty() {
        return Err(AmsError::Empty);
    }
    if bytes.len() % RECORD_LEN != 0 {
        return Err(AmsError::BadSize(bytes.len()));
    }
    Ok(bytes
        .chunks_exact(RECORD_LEN)
        .map(|rec| Specimen {
            name: text_at(rec, OFF_NAME, NAME_LEN),
            field_am: f32_at(rec, OFF_FIELD),
            freq_hz: f32_at(rec, OFF_FREQ),
            instrument: text_at(rec, OFF_INSTRUMENT, 8),
            volume_cm3: f32_at(rec, OFF_VOLUME),
            orient: crate::orient::read(rec),
            georot: crate::georot::read(rec),
            inphase: parse_phase(rec, OFF_KRE),
            outphase: parse_phase(rec, OFF_KIM),
        })
        .collect())
}

pub fn read_file(path: &std::path::Path) -> Result<Vec<Specimen>, Box<dyn std::error::Error>> {
    Ok(parse(&std::fs::read(path)?)?)
}

/// Build one 640-byte record. Used by the tests and the demo-data generator;
/// it writes only the fields this crate reads.
pub fn build_record(name: &str, phase_in: &Phase, phase_out: &Phase, field: f32, freq: f32) -> Vec<u8> {
    build_record_with(name, phase_in, phase_out, field, freq, "DEMO")
}

/// Like [`build_record`] with the instrument text of the record given.
pub fn build_record_with(name: &str, phase_in: &Phase, phase_out: &Phase, field: f32, freq: f32, instrument: &str) -> Vec<u8> {
    let mut r = vec![0u8; RECORD_LEN];
    for (i, b) in name.bytes().take(NAME_LEN).enumerate() {
        r[OFF_NAME + i] = b;
    }
    r[OFF_FIELD..OFF_FIELD + 4].copy_from_slice(&field.to_le_bytes());
    r[OFF_FREQ..OFF_FREQ + 4].copy_from_slice(&freq.to_le_bytes());
    for (i, b) in instrument.bytes().take(8).enumerate() {
        r[OFF_INSTRUMENT + i] = b;
    }
    r[OFF_VOLUME..OFF_VOLUME + 4].copy_from_slice(&10.0f32.to_le_bytes());
    for (base, p) in [(OFF_KRE, phase_in), (OFF_KIM, phase_out)] {
        let mut put = |off: usize, v: f64| r[base + off..base + off + 4].copy_from_slice(&(v as f32).to_le_bytes());
        put(0, p.km);
        for i in 0..3 {
            put(20 + 4 * i, p.sd[i]);
            put(128 + 4 * i, p.ftest[i]);
        }
        let t = &p.tensor;
        for (i, v) in [t[1][1], t[0][0], t[2][2], t[0][1], t[0][2], t[1][2]].into_iter().enumerate() {
            put(32 + 4 * i, v);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_phase(km: f64) -> Phase {
        Phase {
            km,
            sd: [1e-3, 1e-3, 1e-3],
            tensor: sym(1.2 * km, 1.0 * km, 0.8 * km, 0.02 * km, 0.0, -0.01 * km),
            ftest: [10.0, 2.0, 3.0],
        }
    }

    #[test]
    fn roundtrip_record() {
        let mut bytes = build_record("ZZ01-A1", &demo_phase(1e-3), &demo_phase(1e-5), 400.0, 1220.0);
        bytes.extend(build_record("ZZ01-B1", &demo_phase(2e-3), &demo_phase(2e-5), 400.0, 1220.0));
        let s = parse(&bytes).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "ZZ01-A1");
        assert_eq!(s[0].instrument, "DEMO");
        assert!((s[0].freq_hz - 1220.0).abs() < 1e-6);
        assert!((s[1].inphase.km - 2e-3).abs() < 1e-9);
        let ax = s[0].inphase.axes();
        assert!(ax.k_norm[0] > ax.k_norm[1] && ax.k_norm[1] > ax.k_norm[2]);
        assert!(((ax.k_norm[0] + ax.k_norm[1] + ax.k_norm[2]) / 3.0 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rotating_a_record_in_place_equals_rotating_its_tensors() {
        let mut b = build_record("ZZ01-A1", &demo_phase(1e-3), &demo_phase(1e-5), 400.0, 1220.0);
        let before = parse(&b).unwrap().remove(0);
        let r = crate::coords::owens_rotation(30.0, 20.0, 45.0, 10.0);
        rotate_record(&mut b, &r);
        let after = parse(&b).unwrap().remove(0);
        let want = crate::tensor::rotate_tensor(&before.inphase.tensor, &r);
        for i in 0..3 {
            for j in 0..3 {
                assert!((after.inphase.tensor[i][j] - want[i][j]).abs() < 1e-6);
            }
        }
        assert_eq!(after.name, "ZZ01-A1");
        assert!((after.inphase.km - before.inphase.km).abs() < 1e-9);
        assert!((after.outphase.axes().k_norm[0] - before.outphase.axes().k_norm[0]).abs() < 1e-5);
    }

    #[test]
    fn rejects_bad_sizes() {
        assert!(matches!(parse(&[0u8; 641]), Err(AmsError::BadSize(641))));
        assert!(matches!(parse(&[]), Err(AmsError::Empty)));
    }
}
