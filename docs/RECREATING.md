# Recreating this project quickly

This is the playbook for someone (or an LLM) who wants to redo or extend this work, written after doing it once. It says what worked, what did not, and the traps. Read `AGENTS.md` first for the repository map, then `ORIGINAL_BEHAVIOUR.md` (the decoded behaviour) and `FORMATS.md`.

## 0. The fast path

If you only want the software: the `crates/` tree is complete and `cargo test` is green. The expensive part of this project was **finding out what the original does**; that knowledge is in `ORIGINAL_BEHAVIOUR.md` and `FORMATS.md`. If you have to extend it (per-specimen orientation, import of `.ran`/`.asc`, a new tool), use the method in section 3: generate made-up input, run the original, compare, derive the rule, write a regression test from the original's printed numbers.

## 1. What was built

A Rust workspace:

- `ams-core`: no GUI. `.ams` reader (`ams`), eigen-decomposition and parameters (`tensor`), coordinate systems (`coords`, approximate), group statistics with Jelinek ellipses and bootstrap (`stats`), contour density and contour lines (`density`), `.ged` files (`ged`), the four tool calculations (`tools`), csv export (`export`). Examples: `validate` (decoded `.ams` vs the original's csv), `check_tools` (tool results vs files the original wrote), `make_demo` (made-up data).
- `ams-studio`: the egui/eframe app. `app.rs` (state, menus, toolbar, plots, windows, shortcuts), `app/tools_ui.rs` (five tool windows), `app/analysis_ui.rs` (Group Statistics and Contour), `plots.rs` (stereonet, histogram, scatter with rubber band), `icons.rs` (vector toolbar icons, no image assets), `doc.rs` (the open file: records, active/selected flags, display order).

Stack choices that mattered: Rust + egui because it gives resizable/collapsible panels and a single native binary on Linux and Windows; no third-party plotting crate (everything is drawn with the egui painter, which made rubber-band selection, per-marker picking and PNG export of exact regions easy); `rfd` for native file dialogs; `image` only to write PNGs.

## 2. Order of work that would be fastest now

1. Core maths from `FORMATS.md` and `ORIGINAL_BEHAVIOUR.md` section 2; check against an original csv with the `validate` example (agreement to 1e-7 on every column).
2. App shell: panels, plots, list, status bar, menus/toolbar in the original's order. Keep the toolbar order, the shortcut table and the coloured status bar.
3. Interaction: selection model first (click, shift/ctrl, drag, rubber band, marker-type modifiers), because every plot and list depends on it.
4. Windows: Data Table, Group Statistics, Contour, then the five tools, each with a test taken from the original's printed numbers.
5. Polish: dark theme, persistence of layout, big popups.

## 3. How the original was studied (method)

### 3.1 Getting at the program

- The installer (`Anisoft6-Setup.exe`, version 6.1.02, 3 MB) is **Inno Setup 5.5.7**; `innoextract` unpacks it on Linux without Windows (on Fedora: `dnf download innoextract` then `rpm2cpio ... | cpio -idm` to avoid installing). Result: `app/Anisoft6.exe` (2.6 MB), `app/pics`, `app/sounds`, and VB6 runtime/OCX files in `sys/`.
- `Anisoft6.exe` is a **Visual Basic 6** native-code program (it imports `msvbvm60.dll`, controls come from OCX files). That decides the approach: decompiling VB6 native code to readable C is poor, but VB6 executables keep rich metadata.
- Static structure (Python, 20 lines): the `VB5!` header sits at file offset `0x7814`. Header fields: `+0x30` pointer to the project data, `+0x44` form count (24), `+0x4C` pointer to the GUI table. Project data `+4` -> object table; object table `+0x2A` total objects (47), `+0x30` pointer to the object array of 0x30-byte descriptors whose field `+0x18` is the object name pointer and `+0x28` the type (`0x18083` = form, `0x18001` = module, `0x118003` = class). GUI table entries are 0x50 bytes: form blob size at `+0x40`, blob pointer at `+0x48`. Pointers are virtual addresses; convert with the PE section table (image base 0x400000).
- Useful result: the **list of object names** (`frmGroupStat`, `frmContour`, `frmDeformAMS`, `frmFabricOverprint`, `frmCompositeFabric`, `frmFoliLine`, `frmGEDEditor`, `frmOPAR`, `frmSettingAMU1`, modules `Anisoft`, `modAniso`, `aaMatrix`, `BootStat`, `Diagrams`, ...) and the **captions** (ASCII strings in each form blob: button texts, labels, tooltips, e.g. the OP1..OP4 explanations). The binary property stream (positions, colours) was not decoded; positions came from screenshots.
- **REA + Ghidra**: [REA](https://github.com/morluto/rea) (`npm install rea-agents`, Node >= 24.22.1 and a full JDK 25) driving Ghidra 12.x (`GHIDRA_INSTALL_DIR`) analysed and decompiled the exe (`rea analyze <exe> --provider ghidra`, `rea decompile-all <exe> --output <new dir>`; the output directory must not exist). Do not run `rea setup`: it rewrites your agent configuration. Honest assessment: this gave the program overview and a function inventory but **did not yield the algorithms** (VB6 compiled code is dense and unnamed). Every number in this project came from black-box experiments, which were faster and exact.

### 3.2 Black-box calibration rig (the part that worked)

1. Run the original in a Windows VM. Drive it headlessly from the Linux host: `xfreerdp` into a private `Xvfb` display, `xdotool` for mouse/keyboard, `import` (ImageMagick) for screenshots, RDP drive redirection (`/drive:data,<folder>` appears as `\\tsclient\data`) to hand files in and out. Files written by the original (`Save Active Records As`, GED `Save As`, tensor tool `Save As`) land back in that folder.
2. Make **made-up input** with known structure (`make_demo`), not real data.
3. Do one thing in the original, save the result, and decode the saved bytes or read numbers off the screen; guess the rule; test it in Python on the same input; accept it only when it matches to display precision on a **second** input with a different N or settings.
4. Turn the original's printed numbers into a Rust regression test (see `stats.rs`: 12 made-up tensors and the exact values the original printed).

Discoveries made this way, to show the style: the stored tensors are normalised and Km separate (byte diff of saved files); tensor addition is `|Km|`-weighted (least-squares fit of the saved tensors, then the negative-Km records exposed the absolute values); the Jelinek angles needed the covariance-ellipse form (pairwise formula gave K1 5.2 vs K3 5.4 where the original has asymmetric numbers, then the fit constant came out as sqrt(N)); the Deform AMS grain lattice (searched lattice sizes/offsets until one deformation reproduced the fabric to 1e-6, then 23 strain combinations were captured and the matrices read off); the geographic rotation (varied OP1/OP2/OP3 one at a time and fitted a fixed rotation to the output directions).

### 3.3 Traps of the rig (each cost minutes)

- The VM's keyboard is German: typed text arrives scrambled (Y/Z swapped, punctuation moved). Click dialogs with the mouse, and type only digits.
- A virtiofs share (`Y:`) did not show files created after the VM booted; RDP drive redirection did.
- The original opens at its last-used folder: **check the file dialog path before opening anything** so you do not load real data by accident.
- Esc closes the tool windows. Dropdown lists must be dismissed by clicking elsewhere, not Esc.
- The original's windows open at different positions each time: take a full screenshot before clicking coordinates.
- Default view is GEO, so files without orientation data look empty (NGEO = 0) until you press S.
- A shift-click range in its specimen list did not select a range; use it only for highlighting. Group statistics ignore the selection; to change N use a smaller file or Disregard.
- `pkill -f <name>` matches your own shell's command line and kills it: use `pkill -x`.
- No OCR was available; read screenshots by eye, crop and enlarge small regions (`magick ... -crop WxH+X+Y -filter point -resize 200%`).
- A long RDP session times out when the host is idle; reconnect and relaunch the program.

## 4. Architecture notes for the app

- `App` (in `app.rs`) holds the document (`Doc`), the derived data (`Vec<Derived>`: rotated tensor, axes and parameters for the current phase and coordinate system, recomputed when `dirty`), view state (phase, coordinate system, plot dividers, panel widths, colour coding, log Km), window flags, and the analysis state. Everything is immediate mode: each frame draws the panels, then the plots, then the windows, then runs the queued `Action`s.
- **Actions**: menus, toolbar buttons and shortcuts push an `Action`; `run()` executes them after the frame. New features: add an `Action` variant, a menu/toolbar entry, a shortcut row, and a branch in `run()`.
- **Selection**: `doc.selected[i]`; `doc.active[i]` is "not disregarded". `doc.order()` is the display order (active first, disregarded last) used by the list and the Data Table. Plot rubber bands return specimen indices; Ctrl/Alt/Shift choose K1/K2/K3 markers (`plots::axis_mask`).
- **Windows** use `base_window(title, slot, size)`: the egui id contains a per-slot serial that is bumped each time the menu item is used, which makes a reopened window start at the default centred position and size every time. `pop_style(ui, factor)` scales text, spacing and buttons for the big popups; `zf(ui)` reads the factor back so painter-drawn text can follow.
- **Layout persistence**: panel widths and the window size are read each frame; a change stamps `layout_changed`, and `track_layout` writes `state.txt` half a second after the last change and only when no mouse button is down (so dragging does not hammer the disk). The window size is restored at launch through `ViewportBuilder` before the app exists (`saved_window()`).
- **Screenshots/copy**: `ctx.request_screenshot(callback)` fills a shared slot; the next frame crops the stored rectangles. Copy Graphics, Save Graphics and the tools' Copy Graphics all use it.
- **Stats in the UI are recomputed every frame** while a window is open (bootstrap of 500 resamples is a few ms). If you add something heavy, cache it keyed on the specimen list, phase, coordinate system and smoothing as `contour_window` does.

### egui 0.37 specifics (the API moved; old tutorials will not compile)

- `eframe::App::ui(&mut self, ui, frame)` is the entry point; panels are `egui::Panel::left/top/bottom(..)` and `egui::CentralPanel`, shown with `.show(ui, ..)`.
- `ui.id()` is `ui.scope_id()`; `Id::new` is `Id::unique`; `SelectableLabel` is `Button::selectable(sel, text)`; menus are `egui::MenuBar::new().ui(ui, ..)`; styles via `ctx.global_style_mut`; the screen rectangle is `ctx.content_rect()`.
- `Context::request_screenshot(callback)`, `copy_image`, `copy_text`; scroll state `.state.clamped_offset()`.
- `consume_key(mods, key)` ignores **extra Shift/Alt** but not Ctrl: register the most specific shortcut first, and plain letters (Q, E, W) fire with Shift/Alt held. Skip shortcuts while `ctx.egui_wants_keyboard_input()` so typing in fields works.
- `TextEdit::desired_width` is capped by the grid column's current width: wrap grid cells in `ui.scope` with `set_min_width`.
- A drag's `interact_pointer_pos()` is the *current* pointer; use `input.pointer.press_origin()` for where it started.
- Indexing `derived[i]` right after loading a different file is a bug (the panel code runs in the same frame): `open_path` calls `refresh()` immediately and the plot code checks lengths.

## 5. Testing

- `cargo test` (42 tests): core maths, record and `.ged` round trips, the 12-specimen regression against the original's printed output, strain matrices, contour levels and lines, GUI state helpers.
- `cargo run -p ams-core --example validate -- file.ams export.csv` and `... --example check_tools -- A.ams B.ams added.ams subtracted.ams overprint.ams` repeat the comparisons against the original's own output on your files.
- **Headless GUI testing** (what produced all screenshots): `Xvfb :98 -screen 0 1500x1000x24`, then launch with `env -u WAYLAND_DISPLAY DISPLAY=:98 LIBGL_ALWAYS_SOFTWARE=1 HOME=<fresh dir> XDG_CONFIG_HOME=<fresh dir>/.config ./target/release/ams-studio file.ams`, drive with `xdotool` (`windowfocus` first; drags need `mousedown`, several `mousemove`s with short sleeps, `mouseup`), capture with `import -window root`. A fresh `XDG_CONFIG_HOME` gives the default layout; write `win_w=1400`/`win_h=950` into its `state.txt` for a bigger test window. Native file dialogs cannot be tested this way; everything behind them is covered by unit tests.

## 6. Building and releasing

- `cargo build --release -p ams-studio` gives one binary (no installer, no runtime files). On Linux the build needs no extra libraries; running needs a Wayland or X11 session.
- `.github/workflows/release.yml` builds the Windows `.exe` (`windows-latest`, MSVC) and a Linux binary on every `v*` tag and attaches them to a GitHub Release; `ci.yml` runs the tests on every push. The Windows build hides the console window (`windows_subsystem = "windows"` in `main.rs`) and reads its preferences from `%APPDATA%`.

## 6b. Windows build without Windows

The `.exe` is cross-compiled on Linux in a container: `podman run` of `rust:latest`, `apt install gcc-mingw-w64-x86-64`, `rustup target add x86_64-pc-windows-gnu`, `cargo build --release --locked --target x86_64-pc-windows-gnu -p ams-studio` with `RUSTFLAGS='-C link-arg=-static-libgcc -C link-arg=-static'` (no MinGW DLLs needed). It runs on Windows 10/11 and was tested by double-clicking it in a Windows 11 VM (opens files, draws plots, native dialogs work). It is unsigned. Copy a clean source tree into the container (not `target/`).

## 7. Open items (what to do next)

1. **Per-specimen orientation**: Edit Data should read a `.ged`, write OP1-OP4/alpha/phi/foliation fields into the `.ams` records (offsets in `FORMATS.md`) and G/P/T should use them. The geographic and paleo rules are in `ORIGINAL_BEHAVIOUR.md` section 9; the tectonic rule and OP4 are undecoded. The global Orientation Parameters window is an approximation to be replaced.
2. Contour `Foliation Pole` / `Lineation` inputs need that data.
3. Import of `.asc`/`.ran`, Set Data Classes, Paleo-Horizontal/Vertical: menu items exist, behaviour not implemented.
4. Per-specimen E12/E23/E13 are approximate (5 %). Bootstrap ellipse numbers differ from the original's; the contour maximum is within about 3 % (kernel `exp(k (cos^2 - 1))` is close, not identical); contour level steps for maxima above 45 are guessed.
5. Find Duplicates in the tensor tool keeps the first record of each name; the original's rule is unknown.
6. Deform AMS should start from the main window's fabric as the original does.
