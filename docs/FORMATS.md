# File formats

## `.ams` (AGICO Safyr7 binary)

Fixed **640-byte records, no file header**; record count = file size / 640. All numbers little-endian float32 unless stated. Verified by decoding files written by Safyr7 and by Anisoft itself.

| offset | size | content |
|---|---|---|
| 0 | 24 | specimen name, ASCII, space/NUL padded |
| 28 | 4 | field strength (A/m) |
| 40 | 4 | frequency (Hz) |
| 44 | 8 | instrument text (e.g. `KLY5`) |
| 52 | 8 | program text (e.g. `Safyr7`; `SimulDat` for Fabric Overprint output) |
| 68 | 4 | sample volume in cm3 (the Volume column of Anisoft's Edit Data); 10.0, the reference volume of the Kappabridge, in every record seen. AMS Studio's Volume Correction writes the real sample volume here; the shape (cubic / cylindrical) has no place in a record and goes to `<file>.volume.json` next to the file (when the rows of a file got different volumes, the json also lists `record_volumes_cm3` and `record_shapes`, row 1 first) |
| 82, 84, 86, 88 | 2 each | int16 orientation parameters OP1..OP4 (default 6, 0, 6, 0) |
| 90 | 2 | int16 `-1` when the record carries GED orientation data |
| 92, 96 | 4 each | sampling azimuth alpha and plunge phi (float32) |
| 100 | 4 | foliation #1 code, space, foliation #2 code, space (ASCII) |
| 108, 112 | 4 each | azimuth of foliation #1, #2 (float32) |
| 124, 128 | 4 each | dip of foliation #1, #2 (float32) |
| 140 | 4 | lineation #1 code, space, lineation #2 code, space (ASCII) |
| 148, 152 | 4 each | trend of lineation #1, #2 (float32) |
| 164, 168 | 4 each | plunge of lineation #1, #2 (float32) |
| 180 | 144 | **in-phase block** |
| 324 | 144 | **out-of-phase block** (identical layout) |
| 616 | 24 | AMS Studio's **reorientation block** (zero in all 1432 records of the project's own files, so the padding is free): the text `AMSG` (616), version 1 (620), method (621: 1 drill core, 2 Owens), face and arrow indices (622, 623; drill core only), then four float32 at 624: drill core `dip direction, plunge, offset, 0`, Owens `FS, FD, DS, DD`. The program computes the geographic view from it (`ams_core::georot`); the tensors are never rotated. Other software ignores it (Anisoft was not tested with such a file) |

Block layout (offsets relative to the block start):

| +0 | Km, bulk susceptibility (not volume corrected) |
|---|---|
| +8..+16 | K1, K2, K3 floats: **not reliable** (stale in some files); recompute from the tensor |
| +20..+28 | K1, K2, K3 standard deviations |
| +32..+52 | tensor in the order **Tyy, Txx, Tzz, Txy, Txz, Tyz**, normalised to trace 3 for the in-phase block |
| +128..+136 | F-test values F, F12, F23 |

Rules that cost time to find:

- Recompute eigenvalues/vectors from the tensor: build `T = [[Txx,Txy,Txz],[Txy,Tyy,Tyz],[Txz,Tyz,Tzz]]` (note the stored order is Tyy, Txx, Tzz, ...), eigen-decompose, sort descending, flip each eigenvector so that `v[2] >= 0`, then `inc = asin(v[2])` and `dec = atan2(v[0], v[1])`, in degrees, `dec` mod 360. This reproduces Anisoft's printed values to 0.002 degrees.
- Normalised eigenvalues (mean 1) are what Anisoft prints as K1/K2/K3; absolute eigenvalues are not meaningful because the stored tensor is normalised.
- `L F P Pj T U Q E` are not stored; compute them (ORIGINAL_BEHAVIOUR.md section 2). Anisoft writes -2.0 in its csv when `k3 < 0`.
- Out-of-phase Km can be negative; its tensor is not trace-normalised.
- Duplicate specimen names occur (stale records from corrections); no internal way to tell which is current.
- Saving a subset (Save Active Records As, Append Selected Records) copies whole 640-byte records, so nothing is lost.

## `.ran` (Anisoft 4.2 / older AGICO binary, read only for reference)

Decoded from files made up for the purpose; the original Anisoft 6 cannot write it. A 64-byte header (a backslash, then spaces, the text `6  0  6  0` at offset 0x30) followed by 64-byte records:

| offset | content |
|---|---|
| 0 | name, 12 bytes, space padded |
| 12 | 4 zero bytes |
| 16 | Km, float32, in units of 1e-6 |
| 20 | six float32: Txx, Tyy, Tzz, Txy, Tyz, Txz, trace-normalised, axes x north, y east, z down |
| 44 | `SS` |
| 52 | `00` |

Anisoft 4.2 reads `.ran`, older text `.ams` and a few others, and rejects Safyr7 `.ams` files. AMS Studio imports `.ran` (File > Import Data File(s); no out-of-phase data, standard deviations or F-tests come with it) but does not write it; its *Spec to Geo (Owens)* works on `.ams` directly.

## `.ged` (GED Editor output)

CRLF text, every line padded with spaces to 62 characters.

Line 1: `rows + 2` (width 4, left aligned), site name (16), latitude (7), longitude (7), rock type, strat, litho, region (4 each), OP1..OP4 (3 each).

Then one line per specimen: name (12), sampling dec (4), inc (4), `fol1-code` and `lin1-code` as two characters in a 4-wide field, fol1 dec (4), fol1 dip (4), lin1 trend (4), lin1 plunge (4), `fol2-code` and `lin2-code` (4-wide field), fol2 dec (4), fol2 dip (4), lin2 trend (4), lin2 plunge (4). Empty cells are written as `0`. `crates/ams-core/src/ged.rs` parses and writes it and its test reproduces a file written by the original byte for byte.

## csv exported by the original

Columns: `Site,Specimen,Field,Freq,Km,K1,K1_SD,K2,K2_SD,K3,K3_SD,K1_Dec,K1_Inc,K2_Dec,K2_Inc,K3_Dec,K3_Inc,L,L_SD,F,F_SD,P,P_SD,Pj,Pj_SD,T,T_SD,U,U_SD,Q,Q_SD,E,E_SD,FTest_F,FTest_F12,FTest_F23,E12,E23,E13`. The out-of-phase csv repeats the in-phase Km and SDs, so it is wrong for those columns; the `.ams` is the authority.

This program's export (`File > Export Data File` and `Export Selection`, also `cargo run -p ams-core --example export_csv -- file.ams`) writes **one row per specimen with both phases**: `Specimen,Field,Freq`, then `ip_` + (`Km,K1,K1_SD,K2,K2_SD,K3,K3_SD,K1_Dec,K1_Inc,K2_Dec,K2_Inc,K3_Dec,K3_Inc,L,F,P,Pj,T,U,Q,E,FTest_F,FTest_F12,FTest_F23,E12,E23,E13,Tyy,Txx,Tzz`), the same 30 columns with the `op_` prefix, and finally `opKm_over_ipKm_percent` (64 columns). Specimen coordinates, Km as stored, undefined parameters empty, K1-K3 normalised to mean 1, `Tyy/Txx/Tzz` the stored diagonal tensor components, E12/E23/E13 approximate.

## Preferences file

`~/.config/ams-studio/state.txt` on Linux (`%APPDATA%\ams-studio\state.txt` on Windows): `key=value` lines: `last_dir`, `dark`, `browser`, `hfrac`, `vfrac`, `logkm`, `marker` (plot marker size, default 6; remembered), `browser_w`, `spec_w`, `win_w`, `win_h`, `maximized`. Delete it to reset the layout.

## Demo files

`examples/demo_60_specimens.ams` and `examples/demo_12_specimens.ams` are **made up** by `cargo run -p ams-core --example make_demo -- out.ams N` (fixed seed, two sites, realistic normalised tensors). The 12-specimen file with its Group Statistics result printed by the original is used as a regression test in `stats.rs`.

## `<file>.volume.json` (written by AMS Studio)

Next to an `.ams` file whose volume was entered with Edit > Volume Correction: `{"file": "...", "volume_cm3": 3.375, "shape": "cubic" | "cylindrical", "applied": "YYYY-MM-DD", "records": n}`.

## csv columns of AMS Studio's export

`Specimen,Field,Freq`, then the same columns for the in-phase (`ip_`) and the out-of-phase (`op_`) data, then `opKm_over_ipKm_percent`. After the columns of the original's csv (`Km`, `K1`..., `Tyy,Txx,Tzz`) come the M3ORE lab sheet's: `Km_vol` (Km x 10 / volume), `K1_un K2_un K3_un` (unnormalised), `L_Khan F_Khan H_Owens`, `Lin_Plunge Fol_Dip Fol_DipDir`, and the lab sheet's errors: `K1_frac K2_frac K3_frac` (SD / K), `Km_frac`, `K1_un_frac ...`, and the fractional (`_frac`) and absolute (`_abs`) errors of L, L_Khan, F, F_Khan, P, H, Pj, T, U, computed with the sheet's rules (`ams_core::lab`). The standard deviations stored in the file are used as the absolute errors of the normalised K; the percent error of Km is 0 (the sheet's optional input).
