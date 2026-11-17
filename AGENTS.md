# Notes for coding agents (and humans in a hurry)

**What this is:** a Rust reimplementation of AGICO Anisoft 6 (anisotropy of magnetic susceptibility, AMS) with the same menus, toolbar order and results, plus a modern UI. Windows and Linux, one executable.

## Read in this order

1. `README.md`: what the program does and how it differs from the original.
2. `docs/ORIGINAL_BEHAVIOUR.md`: every rule recovered from the original (formulas, windows, what is verified and how closely).
3. `docs/FORMATS.md`: `.ams` record layout, `.ged`, csv, preferences file.
4. `docs/RECREATING.md`: how the original was studied, the traps, architecture, egui 0.37 gotchas, testing rig, open items.

## Repository map

```
crates/ams-core/   pure Rust, no GUI, the testable part
  src/ams.rs           .ams reader/writer (640-byte records), Axes (eigen + parameters)
  src/tensor.rs        eigen decomposition (Jacobi), dec/inc, L F P Pj T U Q E
  src/stats.rs         group statistics, Jelinek ellipses, bootstrap
  src/density.rs       contour density, levels, contour lines, orientation eigenvalues
  src/tools.rs         tensor add/subtract, fabric overprint, intersection, Deform AMS
  src/ged.rs           .ged parse/write
  src/coords.rs        S/G/P/T rotations (approximate, global parameters)
  src/export.rs        csv export
  src/params.rs        the 24 plottable parameters (PARAM_LABELS) and their values per specimen
  examples/            validate, check_tools (compare with the original), make_demo
crates/ams-studio/        egui app
  src/app.rs           state, menus, toolbar, plots layout, windows, shortcuts, persistence
  src/app/gis_*.rs     GIS viewer (View menu): gis_data (tiles, maps, tables), gis_vector (shapefile / GeoPackage / GeoJSON readers, tested), gis_collar (drill core: collars, depth order, box placement; tested), palette (the Ctrl+Shift+P command list and its filter; tested), gis_qgis (reads a QGIS project's legend, layer paths, filters, labels and systems, no data; tested), gis_filter (the QGIS filter syntax as an expression engine over attribute rows; tested), gis_ui (viewer), gis_georef (table + georeference windows + CRS picker), gis_loc / gis_loc_ui (per-specimen places in the dependency file, Specimen locations window), gis_manual_ui (Enter coordinates), gis_layers (the layer tree of the GIS pane, tested) and gis_layers_ui (the layers sidebar), prefs_ext (text forms of pane settings); pane_layout.rs (pane order, rows and rectangles of the main window, tested without a window); maths in ams-core/src/gis.rs
  src/app/spec_view.rs Sort / filter of the Sub-specimens list
  src/app/tools_ui.rs  GED Editor, Tensor Addition/Subtraction, Fabric Overprint, Intersection, Deform AMS
  src/app/analysis_ui.rs  Group Statistics and Contour Plot
  src/plots.rs         stereonet / histogram / scatter drawing + rubber-band picking
  src/icons.rs         vector toolbar icons
  src/icon.rs          the application icon (AMS ellipsoid), drawn procedurally; `ams-studio --write-icons assets` regenerates assets/icon.png/.ico
  build.rs             embeds assets/icon.ico into the Windows exe
  src/doc.rs           open document (records, active/selected, order)
examples/*.ams         made-up demo data
docs/                  documentation and screenshots
```

## Commands

```
cargo test                                   # 42 tests
cargo run --release -p ams-studio -- examples/demo_60_specimens.ams
cargo run -p ams-core --example make_demo -- out.ams 60
cargo run -p ams-core --example validate -- file.ams anisoft_export.csv
```

## Rules that matter

- **Numbers come from the original, not from theory.** Do not "improve" a formula (for example by switching the Jelinek angles to an F-distribution form) without re-checking against the original's output. Regression tests hold the original's printed numbers.
- **Never use real measurement data in the repository** (tests, examples, screenshots, docs). Use `make_demo`. `*.ams` is git-ignored except `examples/*.ams`.
- Stored tensors are normalised (trace 3), Km is separate, and group statistics use the stored tensors as they are. Vector order and `dec`/`inc` conventions are in `docs/ORIGINAL_BEHAVIOUR.md`; use `tensor::dec_inc` / `from_dec_inc`.
- Keep the original's menu order, toolbar order and shortcuts. New features are additions (Q/E/W keys, Log Km, file browser, rubber band, dark theme, Export Selection), never replacements.
- Windows must reopen identically: use `base_window`, never `egui::Window::new` directly for a new popup.
- Do not modify `reference/` (git-ignored, may hold screenshots of the original).
- `pkill -f` kills your own shell command line; use `pkill -x` when cleaning up test processes.

## Testing the GUI headlessly

See `docs/RECREATING.md` section 5 (Xvfb + xdotool + a fresh `XDG_CONFIG_HOME`). File dialogs cannot be driven that way.

## Headless test hooks

- `ams-studio <folder>` opens the folder as the parent of the specimen folders (the Open Folder of Specimens dialog).
- `AMS_STUDIO_PICK_FILE=<path>` makes the "Owens from file" file picker return that path instead of opening a dialog, so the flow can be driven on a private X display (Xvfb with `env -u WAYLAND_DISPLAY`, never on the user's desktop).

- `AMS_STUDIO_OPEN_WINDOW=sort|sort-letter|gis|gis-table|gis-georef|gis-loc|gis-manual|gis-manual-all` opens that window on the first frame; with `gis*`, `AMS_STUDIO_GIS_TABLE=<csv>`, `AMS_STUDIO_GIS_MAP=<image, .shp, .gpkg or .geojson>` stand in for the file dialogs, `AMS_STUDIO_GIS_BASEMAP=1` ticks the basemap (satellite by default) and `AMS_STUDIO_GIS_PAIRS="px,py,lon,lat;..."` pre-fills control points for `gis-georef`. Use only on a private Xvfb with made-up data. `AMS_STUDIO_SET=<folder>` opens a folder of specimens without the dialog, `AMS_STUDIO_GIS_ZONE=<n>` sets the UTM zone of the table draft, `AMS_STUDIO_GIS_SAVE=1` runs *Link and save* once, `AMS_STUDIO_GIS_TYPE="<id>|<first>|<second>[|<depth>[|<dip direction>|<plunge>]]"` (or `"<id>=<a>,<b>[,<depth>[,<dip direction>,<plunge>]];..."` for the list) fills the Enter coordinates window (with `gis-manual` / `gis-manual-all`), `AMS_STUDIO_GIS_YES=1` confirms the list's system, `AMS_STUDIO_GIS_PICK="<id>=<datasheet name>;..."` links like a choice in the drop-down. `AMS_STUDIO_GIS_QGIS=<project>` stands in for the file dialog of the QGIS project import (`AMS_STUDIO_TEST_QGZ=<project> cargo test a_real_project -- --ignored --nocapture` prints a project's legend), `AMS_STUDIO_GIS_DEMO_COLLARS=1` replaces the places by made-up drill core (a collar with seven depths, one with three, one alone, two loose specimens, hole directions), `AMS_STUDIO_OPEN_WINDOW=palette` opens the command palette (with `AMS_STUDIO_PALETTE_QUERY=<text>` typed into it), `AMS_STUDIO_SHOW_GRIPS=1` draws the six-dot pane grips always, `AMS_STUDIO_GIS_DEMO_SYMBOLS=1` replaces the places by a grid of made-up symbols with dips and plunges from shallow to steep. The pane layout can be set through `panes=stereo,p3,gis`, `gis_pane=1` and `core_on=1` in `state.txt` of a fresh `XDG_CONFIG_HOME`.
