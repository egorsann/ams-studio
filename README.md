# Anisoft for Linux

An independent, native Linux reimplementation of AGICO's **Anisoft 6** (anisotropy of magnetic susceptibility, AMS), written in Rust with [egui](https://github.com/emilk/egui).

It keeps the original's window: the same menus, shortcuts and toolbar button order, the Specimens list, the four plots (stereonet, N vs Kmean, P vs Kmean, T vs P) and the coloured status bar. On top of that:

- **Resizable panes**: drag the dividers between the four plots, and the edges of the Specimens list and the file browser.
- **Collapsible file browser** on the left (Ctrl+B), listing `.ams` files; click one to open it.
- A modern look with a light and a dark theme (View > Dark Theme).

![main window](docs/main.png)
![group statistics and contour plot](docs/analysis.png)

(Screenshots use made-up data from `make_demo`.)

## Build and run

```
cargo run --release -p anisoft -- [file.ams]
```

Needs a Rust toolchain; on Fedora the Wayland/X11 libraries come with the desktop. Tests: `cargo test`.

## What works

| Area | Status |
|---|---|
| Open / previous / next / reload `.ams` (Safyr7) | works; previous/next walk the `.ams` files in the folder |
| In-phase and out-of-phase AMS (F3 / F4) | works |
| K1-K3, Dec/Inc, L F P Pj T U Q E, F-tests | works, matches Anisoft's csv export (see below) |
| Four plots, selection, disregard / restore, invert / clear | works |
| Colour-coding by Km, P, Pj or T (rainbow / shades / grayscale) | works |
| Data Table (F2), Group Statistics (F9), Contour Plot (F12) | works (see caveats) |
| Save active records as `.ams`, append selected records into an `.ams`, export CSV | works; subsets are copied byte for byte |
| Copy graphics to clipboard, save graphics (one PNG per ticked plot) | works |
| Edit Data (rename a specimen) | works |
| Coordinates S / G / P / T (F5-F8) | works with user-set orientation parameters, conventions unverified |

## Not implemented yet

Import Data File(s) (`.ran`, `.asc`), Set Data Classes, Paleo-Horizontal / Paleo-Vertical, and the Tools menu (GED Editor, Tensor Addition / Subtraction, Fabric Overprint, Intersection Toolbox, Deform AMS). Their menu items and buttons are present so the layout matches; most show a short "not implemented yet" note.

## Caveats

- Group Statistics: the mean tensor and eigenvalues are standard, but the confidence half-angles use a Student-t approximation, not Jelinek's exact F-based formula, and have not been compared with Anisoft.
- Orientation: `.ams` files do not store core or bedding orientation, so G / P / T use the values in Settings > Orientation Parameters. The rotation conventions (core azimuth/dip, bedding strike/dip, paleo declination) are documented in `anisoft-core/src/coords.rs` and have not been checked against Anisoft's own dialog.
- Km is shown exactly as stored. No specimen-volume correction is applied.
- Contour Plot uses an exponential kernel density, not Anisoft's exact contouring.

## Validation

The `.ams` reader and the tensor maths were checked against a csv exported by the original Anisoft 6 for an 88-specimen file. Over all 88 specimens: Km, K1-K3, L, F, P, Pj, E and the F-tests agree to about 1e-7 (relative), principal directions to under 0.002 degrees, T, U and Q to about 1e-5.

```
cargo run -p anisoft-core --example validate -- file.ams anisoft_export.csv
```

(Specimens are joined on Km, because names can differ between a csv and an `.ams` that was renamed afterwards.)

## Layout

- `crates/anisoft-core`: `.ams` parsing (`ams`), eigen-decomposition and anisotropy parameters (`tensor`), coordinate systems (`coords`), group statistics (`stats`), contour density (`density`), CSV export (`export`).
- `crates/anisoft`: the egui application.
- `crates/anisoft-core/examples/make_demo.rs`: writes a made-up `.ams` for demos and screenshots.

## `.ams` layout

Fixed 640-byte records, no header. Name at byte 0; field (A/m) at 28; frequency (Hz) at 40; instrument text at 44; reference volume (10.0 cm3) at 68; the in-phase block at 180 and the out-of-phase block at 324 (144 bytes each). Inside a block: Km at +0, K1-K3 standard deviations at +20, the raw tensor at +32 in the order Tyy, Txx, Tzz, Txy, Txz, Tyz, and the F-tests at +128. K1-K3 are always recomputed from the tensor; the K1-K3 floats stored at +8 are not reliable.

## Legal

This is an independent reimplementation. It contains no AGICO code, binaries or assets. Anisoft is a program of AGICO, Brno; this project is not affiliated with AGICO.
