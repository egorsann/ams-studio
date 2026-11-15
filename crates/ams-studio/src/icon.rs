//! The application icon: an AMS ellipsoid (a shaded triaxial ellipsoid with its K1 red, K2 green
//! and K3 blue axes) on a dark rounded square. Drawn procedurally at any size.

use image::ImageEncoder;

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn seg_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt()
}

fn over(dst: &mut [f32; 4], c: [f32; 3], a: f32) {
    for i in 0..3 {
        dst[i] = dst[i] * (1.0 - a) + c[i] * a;
    }
    dst[3] = dst[3] + a * (1.0 - dst[3]);
}

/// Colour of one sample at normalised coordinates `(x, y)` in `[-1, 1]` (y down): RGBA, 0..1.
fn sample(x: f32, y: f32, aa: f32) -> [f32; 4] {
    let mut px = [0.0, 0.0, 0.0, 0.0];
    // rounded square background
    let (qx, qy) = (x.abs() - 0.62, y.abs() - 0.62);
    let sd = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - 0.34;
    let inside = 1.0 - smooth(-aa, aa, sd);
    if inside <= 0.0 {
        return px;
    }
    let g = (y + 1.0) / 2.0;
    let bg = [0.10 + 0.05 * (1.0 - g), 0.14 + 0.07 * (1.0 - g), 0.24 + 0.10 * (1.0 - g)];
    px = [bg[0], bg[1], bg[2], inside];

    // ellipsoid, long axis tilted
    let ang = -0.50f32;
    let (ca, sa) = (ang.cos(), ang.sin());
    let (ex, ey) = (x * ca + y * sa, -x * sa + y * ca);
    let (a, b) = (0.78f32, 0.43f32);
    let r2 = (ex / a).powi(2) + (ey / b).powi(2);
    let edge = 1.0 - smooth(1.0 - aa * 2.0, 1.0 + aa * 2.0, r2.sqrt());
    if edge > 0.0 {
        let w = (1.0 - r2.min(1.0)).sqrt();
        // surface normal in the ellipsoid frame, light from the upper left in screen space
        let n = [ex / a, ey / b, w];
        let l = {
            let (lx, ly, lz) = (-0.45f32, -0.55f32, 0.70f32);
            let nrm = (lx * lx + ly * ly + lz * lz).sqrt();
            [(lx * ca + ly * sa) / nrm, (-lx * sa + ly * ca) / nrm, lz / nrm]
        };
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        let d = ((n[0] * l[0] + n[1] * l[1] + n[2] * l[2]) / nl).max(0.0);
        let spec = d.powf(24.0) * 0.55;
        let base = [0.36, 0.62, 0.98];
        let shade = 0.28 + 0.78 * d;
        let col = [base[0] * shade + spec, base[1] * shade + spec, base[2] * shade + spec];
        over(&mut px, [0.04, 0.07, 0.15], edge * (1.0 - smooth(0.90, 1.0, r2.sqrt())) * 0.0 + edge * 0.0);
        over(&mut px, col, edge);
        // thin dark rim for contrast at small sizes
        let rim = smooth(0.90, 1.0, r2.sqrt()) * edge;
        over(&mut px, [0.03, 0.06, 0.14], rim * 0.55);
    }

    // principal axes: K1 (long, red), K3 (short, blue), K2 (green, pointing out of the picture)
    let rot = |u: f32, v: f32| (u * ca - v * sa, u * sa + v * ca);
    let k1 = (rot(-0.90, 0.0), rot(0.90, 0.0));
    let k3 = (rot(0.0, -0.55), rot(0.0, 0.55));
    let k2 = ((-0.20, 0.34), (0.20, -0.34));
    let axes = [
        (k1, [0.96, 0.22, 0.22], 0.050),
        (k3, [0.25, 0.42, 1.00], 0.050),
        (k2, [0.20, 0.82, 0.38], 0.050),
    ];
    for (s, c, wd) in axes {
        let d = seg_dist(x, y, s.0, s.1);
        let t = 1.0 - smooth(wd - aa, wd + aa, d);
        let halo = 1.0 - smooth(wd + 0.015 - aa, wd + 0.015 + aa, d);
        over(&mut px, [0.02, 0.04, 0.10], halo * 0.55);
        over(&mut px, c, t);
        // round end caps
        for e in [s.0, s.1] {
            let dd = ((x - e.0).powi(2) + (y - e.1).powi(2)).sqrt();
            let t = 1.0 - smooth(wd * 1.45 - aa, wd * 1.45 + aa, dd);
            over(&mut px, c, t);
        }
    }
    px[3] = px[3].min(inside);
    px
}

/// RGBA bytes of a `size` x `size` icon (3x3 supersampling).
pub fn rgba(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size * size * 4);
    let aa = 2.0 / size as f32;
    for py in 0..size {
        for pxl in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..3 {
                for sx in 0..3 {
                    let x = ((pxl as f32 + (sx as f32 + 0.5) / 3.0) / size as f32) * 2.0 - 1.0;
                    let y = ((py as f32 + (sy as f32 + 0.5) / 3.0) / size as f32) * 2.0 - 1.0;
                    let s = sample(x, y, aa);
                    // premultiplied accumulation
                    for i in 0..3 {
                        acc[i] += s[i] * s[3];
                    }
                    acc[3] += s[3];
                }
            }
            let a = acc[3] / 9.0;
            for i in 0..3 {
                out.push(if acc[3] > 0.0 { ((acc[i] / acc[3]).clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 });
            }
            out.push((a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

/// A multi-size `.ico` file (PNG-compressed entries).
pub fn ico_bytes(sizes: &[usize]) -> Vec<u8> {
    let pngs: Vec<(usize, Vec<u8>)> = sizes
        .iter()
        .map(|&s| {
            let raw = rgba(s);
            let mut buf = vec![];
            image::codecs::png::PngEncoder::new(&mut buf).write_image(&raw, s as u32, s as u32, image::ExtendedColorType::Rgba8).expect("png");
            (s, buf)
        })
        .collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len();
    for (s, p) in &pngs {
        let d = if *s >= 256 { 0u8 } else { *s as u8 };
        out.extend([d, d, 0, 0, 1, 0, 32, 0]);
        out.extend((p.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += p.len();
    }
    for (_, p) in pngs {
        out.extend(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_the_right_size_and_a_transparent_corner() {
        let px = rgba(64);
        assert_eq!(px.len(), 64 * 64 * 4);
        assert_eq!(px[3], 0, "corner is transparent");
        let mid = (32 * 64 + 32) * 4;
        assert_eq!(px[mid + 3], 255, "centre is opaque");
    }

    #[test]
    fn ico_container_is_well_formed() {
        let ico = ico_bytes(&[16, 32]);
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 2);
        let off = u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]) as usize;
        assert_eq!(&ico[off..off + 4], &[0x89, b'P', b'N', b'G']);
    }
}
