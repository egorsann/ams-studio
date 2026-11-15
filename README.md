# AMS Studio

**AMS Studio is a fast, modern, open application for anisotropy of magnetic susceptibility (AMS) data**, built as a modern alternative to AGICO's Anisoft 6. It reads Safyr7 `.ams` files, gives the same numbers as the original (checked against it), keeps the original's menus, toolbar order and shortcuts, and adds the things the original never had: a **dark mode**, resizable panes, a file browser, drag selection, and a single executable for **Windows and Linux**.

> **Download (Windows):** [`dist/ams-studio-windows-x64.exe`](dist/ams-studio-windows-x64.exe), also attached to the [latest release](../../releases/latest). Double-click it, there is no installer. On Linux build from source (below). Try it with the made-up files in [`examples/`](examples).

| Light | Dark |
|---|---|
| ![light](docs/img/main_light.png) | ![dark](docs/img/main_dark.png) |

## What is better than the original

| | Original Anisoft 6 | AMS Studio |
|---|---|---|
| **Look** | One fixed light look | **Light and dark theme** (View menu, remembered), flat vector icons, rounded cards. The toolbar buttons are in the same places as in the original, so muscle memory carries over |
| **Layout** | Four plots at a fixed split | **Drag the dividers** between the plots, resize the Specimens list and the file browser. Window size, panel widths and dividers are **remembered** between runs |
| **Opening files** | File dialog, previous / next buttons | **Collapsible file browser** on the side (like a code editor): folders, back / up / home, click a file to open it, previous / next file in the folder |
| **Selecting** | Click rows | **Whole-row click**, **drag over rows**, **rubber-band** selection in the stereonet, P-Kmean and T-P plots. Hold **Ctrl / Alt / Shift** to pick only **K1 / K2 / K3** markers. **Esc** clears |
| **Keyboard** | Function keys | Function keys as before, plus **Q / E** previous / next specimen (the list follows), **W** switches in-phase / out-of-phase, **Esc** clears the selection |
| **Axes** | Linear or log per axis | **Log Km** checkbox on the P-vs-Kmean plot; log10 option for every axis and colour scale |
| **Popups** | Tool windows open in a different place every time | About **twice as large**, always open **centred at the same size** |
| **Exports** | CSV of one phase | **One CSV with in-phase and out-of-phase together**: one row per specimen, columns `ip_*` and `op_*` (Km, K1-K3 with SD, directions, L F P Pj T U Q E, F-tests, E12/E23/E13, the raw tensor) plus `opKm_over_ipKm_percent`. **Export Selection (CSV)** (Ctrl+Shift+E) does the same for just the selected specimens |
| **Install** | Windows only; installer that brings the Visual Basic 6 runtime and OCX controls | **One executable**, nothing to install, runs on **Windows and Linux** |
| **Trust** | Closed source | **Open source**, 42 automated tests, results checked against the original on its own output files (see below) |

Also: a new **application icon** (an AMS ellipsoid with its K1/K2/K3 axes); disregarded records **sink to the bottom** of the list and the Data Table, struck through and faded; the colour-coding panel, the Export graphics tick boxes and the status bar colours of the original are kept.

## What it can do

Everything below is in the program today.

- **Open and browse** Safyr7 `.ams` files (in-phase and out-of-phase), step through the files of a folder, reload.
- **Four live plots**, each with the option panel of the original (the arrow in its header):
  - **Every parameter can be plotted**: Kmean, L, F, P, Pj, T, U, Q, E, K1-K3 and their declinations/inclinations, the F-tests and the E12 / E23 / E13 confidence angles. The picker opens as one wide panel with the parameters **grouped by type** in coloured columns.
  - **Axes**: any parameter on X and Y, auto-scale or a fixed range, **linear or log10**, diagonal line, conf. intervals (error bars for K1-K3), connect symbols, caption, legend.
  - **Colour-coding** of the markers by any parameter (rainbow, shades or grayscale, auto or fixed scale, linear or log), with a **colour scale bar under each plot** showing the range and the parameter name.
  - **Stereonet extras**: K1/K2/K3 on or off, magnetic-foliation circles, **mean tensor with Jelinek confidence ellipses and K2-K3 / K1-K3 / K1-K2 planes**, **bootstrap cloud**, connect symbols.
  - **Histogram** of any parameter with adjustable bins. Hover for values, click to select. The marker size is **6 by default** and your last choice is remembered.
- **Coordinate systems** S, G, P, T and the in-phase / out-of-phase switch (F3 / F4 / **W**).
- **Data Table** with all parameters (Km, K1-K3 with directions, P, Pj, T, F-tests...), whole-row selection, disregarded records last.
- **Group Statistics** (F9) with the original's colour-coded tables and a stereonet with data points, mean-tensor axes, **Jelinek confidence ellipses**, optional great-circle planes and a **bootstrap cloud**; display toggles for everything.
- **Contour Plot** (F12) for K1, K2, K3: continuous or discrete levels in the axis colour, adjustable smoothing, contour lines, data points, eigenvectors with their eigenvalue table, colour scale.
- **Tools menu**, all five windows: **GED Editor** (read/write `.ged` files), **Tensor Addition / Subtraction** (pair two files by specimen name, add or subtract tensors, save), **Fabric Overprint** (mix two fabrics, simulate the series), **Intersection Toolbox** (two lineations to a plane, two foliations to a lineation, with the net drawn), **Deform AMS** (grain-population model with seven strain types, step by step).
- **Folder of specimens** (File > Open Folder of Specimens, Ctrl+Shift+O, or the button in the file browser; a folder given on the command line works too): pick the parent folder and every sub-folder with `.ams` files is listed as a specimen with all its files, like the tree of TX Studio. Naming differs between labs, so the identifier of the main file per specimen (the author's end in `_use`) is detected and shown as a sentence ("`_use` is the identifier of the main .ams file in each specimen folder") above the list of the files it picks. If nothing recurs the dialog says so and suggests renaming the files before using the program; *Search for another string* takes a text, matches it again and updates the list so you can check every specimen. Previous / Next then walk the main files. In the tree each specimen folder has a folder icon and a gray name, the file the identifier picked is listed first in bold, the other files follow in a darker shade, the file that is open has a coloured background (so you can follow Shift+Q / Shift+E on the left), and **Shift+Q / Shift+E** jump to the previous / next specimen folder while Q / E step through its sub-specimens.
- **Lab-sheet parameters** (the M3ORE sheet `Template_PrinSus`): the colour-coding picker also offers the volume-corrected Kmean, Khan's L and F, Owens' H, the unnormalised K1-K3, the lineation plunge and the foliation dip and dip direction, each in the group where it belongs and marked `*`, with a footnote. The csv export has them too, plus the lab sheet's fractional and absolute errors of L, F, P, Pj, T, U and the Khan / Owens parameters (columns are listed in `docs/FORMATS.md`).
- **Edit > Volume Correction**: the sample volume (and cubic or cylindrical) is written into the Volume field of the records; the shape and date go to `<file>.volume.json` next to the file after you have been told. Hovering the menu item shows the volume already applied to the open file. Km as shown stays as stored; the starred parameters use 10 / volume.
- **Edit > Recombine Split AMS Files**: joins `.ams` files exported in parts. Drag the parts into the order they follow each other, accept or untick the suggestions (order by specimen ID with A1 B1 C1 A2 ... or A1 A2 B1 ..., leave out identical records, one record per name), tick single records in or out of the combined list, and save a new file; the source files are not changed.
- **Edit**: *Edit Data* (the table of per-specimen orientation: sampling angles, foliations, lineations, OP1-OP4, opened from a `.ged` with record pairing, saved into the records), *Set Data Classes* (custom classes, or by file name / specimen name; colours the list and, with Color-coding set to Data Classes, the plots), invert / disregard / restore / clear selection, **Spec to Geo (Owens)** (see below), **Save Active Records As**, **Append Selected Records Into** (records are copied byte for byte), **Export CSV** (in- and out-of-phase in one file), **Export Selection (CSV)**.
- **Graphics**: copy to the clipboard or save PNGs of the plots you tick under *Export graphics*.

### Screenshots

| Group Statistics (light) | Group Statistics (dark) |
|---|---|
| ![group statistics light](docs/img/groupstats_light.png) | ![group statistics dark](docs/img/groupstats_dark.png) |

| Contour plot (K2, discrete, all overlays) | Deform AMS after three steps (dark) |
|---|---|
| ![contour](docs/img/contour_light.png) | ![deform](docs/img/deform_dark.png) |

![intersection toolbox](docs/img/intersection.png)

(All screenshots use the made-up demo files; no measurement data is in this repository.)

## Same results as the original

The numbers were checked by running the original Anisoft 6 on made-up data and comparing what it printed or wrote:

| Area | Agreement with the original |
|---|---|
| Km, K1-K3, L, F, P, Pj, E, F-tests (88 specimens) | about 1e-7 relative |
| Principal directions | under 0.002 degrees |
| T, U, Q | about 1e-5 |
| Group statistics: mean tensor, Jelinek ellipse angles, shape parameters, averages and standard deviations | **exact** on two files (N = 12 and N = 60) |
| Contour eigenvector table | matches to 5 digits |
| Tensor Addition / Subtraction (86 pairs, both phases) | identical |
| Fabric Overprint series | 1e-7 |
| Intersection Toolbox | exact |
| Deform AMS (prolate and oblate grains, step order, all strain types) | about 1e-6 |
| `.ged` files | written byte for byte like the original |

Each check is reproducible: `cargo run -p ams-core --example validate` and `--example check_tools` compare against files exported by the original, and `cargo test` contains regression tests built from numbers the original printed.

## Not (yet) the same

Honest list of the gaps:

- **Per-specimen orientation.** Geographic coordinates (G) use the orientation stored in each `.ams` record (written by *Edit Data* here or by the original); that rule is verified to 0.1 degree. Paleo (P) and tectonic (T) coordinates and the two Paleo-Horizontal / Paleo-Vertical buttons are not done: they still use the global values in *Settings > Orientation Parameters* (P, T) or are disabled, because the original's rules are only partly decoded (leads and the experiment to finish them are in [`docs/ORIGINAL_BEHAVIOUR.md`](docs/ORIGINAL_BEHAVIOUR.md), section 13).
- **Spec to Geo (Owens)** is not in Anisoft 6, only in Anisoft 4.2 (which needs old `.ran` files). *Edit > Spec to Geo (Owens)* gives it for `.ams` files. A phone-style switch at the top left changes the input: *Classic Owens* (the default, remembered between sessions) is the 4.2 table with one row of values per specimen, a row number column and *Copy 1st row to all*; *New Owens* asks for the four values once, FS / FD (strike and dip of a plane in the field) and DS / DD (strike and dip of the same plane on the drilling table), plus the first and last row they apply to (preset to the whole file). A short notice beside the switch names the mode you changed to. Both show a preview of the file's data at the bottom and an (i) explaining each value. A *From file ...* button below the switch reads the values of many specimens from an `.xlsx` or `.csv` instead: it first asks *This file* (the button appears where you clicked, so the mouse need not move) or *Multiple specimens from this parent tree*, then the file. The window then shows how the file has to look (the columns it needs, ticked off against your file, and for the parent tree the required folder structure next to what was found), and a scrollable list of the records or specimens that will get the correction with the values they get. Headers are recognised by name (`Specimen`, `FS`/`Strike`, `FD`/`Dip`, `DS`/`Drill table strike`, `DD`/`Drill table dip`, also the lab template's `Strike (LHR)` and `Dip Direction`). Strike convention (to be confirmed by the lab): index finger along the strike, thumb to the dip direction, so 0° dips towards 90°; `LHR` columns are used as they stand and a dip direction becomes a strike by subtracting 90°. *This file* rotates the open file like the manual input. *Multiple* changes the file tree, after asking: in each specimen folder the source (main) file is moved into a new sub-folder `_raw`, the corrected file `<name>_corr.ams` is written into a new sub-folder `_corr`, other files stay where they are; nothing is overwritten. *Undo last correction (single)* or *(multiple)* sits next to *Select file ...* afterwards; for multiple it deletes the corrected files and moves the sources back. *Rotate* (the highlighted button) rotates the tensors of the open file (both phases), cumulatively like the original; *Save Active Records As* writes them and *Undo last rotation* takes the last one back. The rotation was rebuilt by driving Anisoft 4.2 with made-up tensors: 16 runs agree to the 0.1 degree the original prints (details in [`docs/ORIGINAL_BEHAVIOUR.md`](docs/ORIGINAL_BEHAVIOUR.md)).
- **Bootstrap ellipses** are this program's own method (500 resamples, 95 % region) and differ from the original's.
- **Contour density** is very close to the original (colour-scale maxima within about 3 %) but not identical.
- **Not exact:** the E12 / E23 / E13 confidence angles are computed from the stored standard deviations and match the original to about 5 % (it uses the full covariance, which the files do not contain). The stereonet's *Foliation* / *Lineation* options need `.ged` data and are not available.
- **Import** reads Anisoft 4.2 `.ran` files and the csv written by Anisoft or AMS Studio; the other lab formats of the original's list (ASC, SI2B, AMSSpin, SushiBar, TorqueZurich, AMSBurgos, IODP, BillMorris, Toulouse, Molyneux) are not decoded. *Data Classes* has the criteria Custom, File name and Specimen name as in the original; its *Aniso mode* criterion gives one class because the field it reads is unknown.
- **Not implemented:** Paleo-Horizontal / Paleo-Vertical, per-specimen P and T coordinates, the stereonet's Foliation / Lineation display and the contour inputs *Foliation Pole* / *Lineation* (all need the paleo rules above). Their items are present, greyed out, so the layout matches.
- Deform AMS starts from the isotropic grain population; the original starts from the fabric of the open data.

## Run it

**Windows 10 / 11 (64-bit):** download [`dist/ams-studio-windows-x64.exe`](dist/ams-studio-windows-x64.exe) (SHA-256 in `dist/ams-studio-windows-x64.exe.sha256`) and double-click it. The executable is not code-signed, so Windows SmartScreen may say "unknown publisher": choose *More info* then *Run anyway*. Preferences are kept in `%APPDATA%\ams-studio\state.txt` (delete it to reset the layout).

**Linux:** build from source (below); the binary needs a Wayland or X11 session. Preferences are in `~/.config/ams-studio/state.txt`.

**From source:** `cargo run --release -p ams-studio -- examples/demo_60_specimens.ams` (a recent stable Rust; no other dependencies).

The program never uses the network.

### Keyboard

| Key | Action | Key | Action |
|---|---|---|---|
| Ctrl+O / P / N / R | open / previous / next file / reload | F3 / F4 / **W** | in-phase / out-of-phase / switch |
| Ctrl+S / A | save active as / append selected | F5 - F8 | S, G, P, T coordinates |
| Ctrl+E / **Ctrl+Shift+E** | export all / **export selection** (CSV) | F9 / F12 | group statistics / contour |
| Ctrl+C / Ctrl+F | copy / save graphics | F2 | data table |
| Shift+Ins / Shift+Del / Alt+Backspace | invert / disregard / restore all | **Q / E** | previous / next sub-specimen (record) |
| | | **Shift+Q / Shift+E** | previous / next specimen folder (its main file) of the folder set, else the previous / next file |
| **Esc** | reset the stereonet zoom, otherwise clear selection | Ctrl+B | show / hide the file browser |

Mouse: click a row or marker to select, **drag over rows** or **drag a rubber band** in a plot to select several; with **Ctrl / Alt / Shift** held the band only picks K1 / K2 / K3 markers. Drag the dividers between plots and the panel edges to resize. **Stereonet:** **Ctrl + scroll** (or a pinch) zooms about the pointer, scroll or middle-drag pans a zoomed net, **Esc** returns to the default zoom; **holding Alt over a marker** shows its sub-specimen ID and axis at once.

## For developers and other LLMs

Everything needed to follow or redo the work is in the repository:

- [`AGENTS.md`](AGENTS.md): repository map, commands, rules.
- [`docs/ORIGINAL_BEHAVIOUR.md`](docs/ORIGINAL_BEHAVIOUR.md): every rule recovered from the original, with how closely it is verified.
- [`docs/FORMATS.md`](docs/FORMATS.md): the `.ams` record layout, `.ged`, csv, preferences.
- [`docs/RECREATING.md`](docs/RECREATING.md): how the original was studied (installer and VB6 structure, REA + Ghidra, the black-box calibration rig), the traps, architecture, egui 0.37 specifics, headless testing, open items.

## Legal

AMS Studio is an independent application written from observed behaviour of AGICO's Anisoft 6. It contains **no AGICO code, binaries, icons or screenshots**. Anisoft and Safyr are products of AGICO, Brno; this project is not affiliated with or endorsed by AGICO. The `.ams` layout was worked out from files; check your own licence terms if that matters for your use. MIT licensed.
