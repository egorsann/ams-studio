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
| 616 | 24 | AMS Studio's **Owens reorientation block** (zero in all 1432 records of the project's own files, so the padding is free): the text `AMSG` (616), version 1 (620), method (621: 2 Owens; 1 drill core is read but the program keeps drill core reorientations in the dependency file instead), face and arrow indices (622, 623), then four float32 at 624: Owens `FS, FD, DS, DD`. The program computes the geographic view from it (`ams_core::georot`); the tensors are never rotated. Other software ignores it (Anisoft was not tested with such a file) |

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

## `.ran` (Anisoft 4.2 / older AGICO binary)

A 64-byte header followed by 64-byte records. Verified against full `.ams` files of the same specimens (214 pairs, 2678 records: tensors and Km identical to float32 precision, `cargo run -p ams-core --example check_ran -- <folder>`).

Header: bytes 0-1 are the record count + 2 as a 16-bit number (like the first line of a `.ged`), then text; its last four numbers are the orientation parameters OP1..OP4 (`6  0  6  0`, or `3  0  3  0`). AMS Studio copies them into the record (offset 82..88).

| offset | content |
|---|---|
| 0 | name, 12 bytes, space padded |
| 12 | 4 bytes: `00 00 af 44` (float32 1400.0) in files made from a measurement with 400 A/m, zero otherwise; not used |
| 16 | Km, float32, in units of 1e-6 SI, **always positive** |
| 20 | six float32 in the order of the `.ams` tensor: Tyy, Txx, Tzz, Txy, Txz, Tyz (x east, y north in this program; the file's first value is north-north), the in-phase tensor trace-normalised to 3 |
| 44 | `SS`, then zeros |

**One phase per file.** The in-phase file is `X.ran`, the out-of-phase file `X_Im.ran` (record names `A2_Im`). The out-of-phase file stores |Km|; the sign is in the tensor: it is not trace-normalised to +3 but to -3 when Km is negative, so Km takes the sign of the trace (right for all 2251 non-empty records checked). `ams_core::import::ran_pair_to_records` joins the two; `ams_core::phases` does the same for two `.ams` files (in-phase file + out-of-phase file, matched by name, the phase word `_Im` / `_oop` / `_out` ... ignored).

Anisoft 4.2 reads `.ran`, older text `.ams` and a few others, and rejects Safyr7 `.ams` files. AMS Studio opens `.ran` directly (File > Open, Open Folder of Specimens with the file type `.ran`, File > Import Data File(s)), joining the `_Im` partner next to it, but never writes `.ran` and never writes into a data folder.

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

## Volume normalisation (Km, K')

Source: AGICO Safyr7 user manual, section 3.2.1 (https://www.agico.cz/downloads/documents/manuals/safyr7-man.pdf).

* The Kappabridge measures the total susceptibility `k_TOT` = induced moment / V0, with the nominal volume of the instrument **V0 = 10 cm3**.
* Safyr7 shows (and, for anisotropy data, always stores) the volume susceptibility **`k_VOL = (V0 / V) * k_TOT`**, `V` being the actual specimen volume typed in for the specimen (the `Act. vol.` line of the `.asc` header; byte 68 of the `.ams` record). Mass-normalised: `chi = (V0 / m) * k_TOT`.
* In this project's data `V` was entered as 10.00 in every record (the `.asc` headers and a scan of all 251 `.ams` files under PhD_work: 7,127 records with 10.0, 170 blank records with 0.0, none with another volume), so Safyr's factor was exactly 1 and the stored Km is `k_TOT`.
* AMS Studio therefore applies the one correction itself, with the real cube volume: `Km_vol = Km * 10 / V_cube` (2.963 for a 15 mm cube, 3.375 cm3; 1.25 for a 20 mm cube, 8 cm3). `K'` = `Km_vol * sqrt(sum (k_i - 1)^2 / 3)` and the unnormalised axes scale the same way; shape parameters do not change.
* A file whose recorded volume is neither 10 nor 0 was already scaled by Safyr (`Km = k_TOT * 10 / V`). Before correcting with a different volume `V'` it must be undone: `Km_vol = Km * (V / 10) * (10 / V') = Km * V / V'`. Studio does not do this automatically; AMS Studio's own Volume Correction also writes its volume into byte 68, with the opposite meaning (Km not yet corrected).

## `<file>.volume.json` (written by AMS Studio)

Next to an `.ams` file whose volume was entered with Edit > Volume Correction: `{"file": "...", "volume_cm3": 3.375, "shape": "cubic" | "cylindrical", "applied": "YYYY-MM-DD", "records": n}`.

## csv columns of AMS Studio's export

`Specimen,Field,Freq`, then the same columns for the in-phase (`ip_`) and the out-of-phase (`op_`) data, then `opKm_over_ipKm_percent`. After the columns of the original's csv (`Km`, `K1`..., `Tyy,Txx,Tzz`) come the M3ORE lab sheet's: `Km_vol` (Km x 10 / volume), `K1_un K2_un K3_un` (unnormalised), `L_Khan F_Khan H_Owens`, `Lin_Plunge Fol_Dip Fol_DipDir`, and the lab sheet's errors: `K1_frac K2_frac K3_frac` (SD / K), `Km_frac`, `K1_un_frac ...`, and the fractional (`_frac`) and absolute (`_abs`) errors of L, L_Khan, F, F_Khan, P, H, Pj, T, U, computed with the sheet's rules (`ams_core::lab`). The standard deviations stored in the file are used as the absolute errors of the normalised K; the percent error of Km is 0 (the sheet's optional input).

## `<specimen folder>_geo_dependancy.json` (drill core reorientation, cube photo link, stereonet planes)

Written by *Save as .ams* into a `_geo` folder, named after that folder: `DC26AB002_AMS_geo/DC26AB002_AMS_geo_dependancy.json`. One entry per `.ams` file of the folder, with one object per record (`null` for a record without a drill core reorientation): `index`, `name`, `dip_direction`, `plunge`, `offset` (the BOH_ROTA of the record's interval), `face` and `arrow`. The `_geo` `.ams` files are exact copies of their sources; the geographic view is computed from this file (`ams_core::depjson`), so it has to stay in the folder with them.

The same file holds two more sections, written by the viewers (`kind` is `AMS Studio dependency file`, `note` describes all of it; the file is removed when nothing is left in it): `core_viewer` has `photo_folder` (absolute) and `photo_folder_relative_to_this_file` (tried first, so it survives moving or syncing the whole data tree); `stereonet` has `planes_specimen` (planes of this specimen only) and `planes_global` (planes of every specimen, a copy in the file of each specimen of the file tree), each plane `{id, on, dip_direction, dip, comment, color}` (dip direction and dip in degrees). The `_geo` folder is made when a link or a plane is stored for a specimen that has none yet.
