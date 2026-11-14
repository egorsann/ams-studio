# What the original Anisoft 6.1.02 does, and how this program reproduces it

Everything below was learned by running the original on made-up data and comparing numbers or saved files, not from AGICO documentation. "Verified" means reproduced to the stated precision against files or screens produced by the original. Where something is only a close match or a guess it says so.

Conventions used throughout:

- Specimen tensors in `.ams` files are stored **normalised** (trace 3), with the bulk susceptibility Km stored separately. Group statistics use the stored tensors as they are.
- In the code, direction vectors are 3-vectors in the tensor's own index order (index 0 is the Txx slot, 1 Tyy, 2 Tzz of the matrix built from the stored values), and `dec = atan2(v[0], v[1])`, `inc = asin(v[2])` after flipping to `v[2] >= 0`. Always go through `tensor::dec_inc` and `tensor::from_dec_inc` instead of re-deriving it.
- Equal-area (Schmidt) net: `r = sqrt(2) sin((90 - inc) / 2)`, point = `(r sin dec, r cos dec)`; X is up and Y to the right for specimen coordinates.

## 1. Main window (layout to preserve)

Title `Anisoft - [path (n / total)]`. Menu: File | Edit | Graphics | View | Analysis | Tools | Settings | About.

One toolbar row, left to right: Open, previous file, next file, reload, save active records as, append selected records, export data; edit data, set data classes; invert selection, disregard selected, restore all, clear selection; copy graphics, save graphics; data table; in-phase, out-of-phase; coordinate buttons **S G P T** (red discs); paleo-horizontal, paleo-vertical; two combo boxes; group statistics, contour plot.

Left "Specimens" list (header arrow opens a colour-coding panel: checkbox, a combo, Rainbow / Shades / Grayscales). Right: 2x2 plots, each with a header arrow and an "Export graphics" checkbox: stereonet, N vs Kmean, P vs Kmean, T vs P. Status bar of 8 coloured cells: NFile (pink), NAct (green), NDis (light red), NGEO (cyan), NSel (yellow), previous file (red), current file (green), next file (blue).

Shortcuts: File: Open Ctrl+O, Previous Ctrl+P, Next Ctrl+N, Reload Ctrl+R, Import Ctrl+I, Save active records Ctrl+S, Append selected Ctrl+A, Export Ctrl+E, Close Ctrl+Q, Exit Ctrl+X. Edit: Edit data F1, Set classes Ctrl+F1, Invert Shift+Ins, Disregard Shift+Del, Restore all Alt+Backspace, Clear selection Shift+Ctrl+F12. Graphics: Copy Ctrl+C, Save Ctrl+F. View: Data table F2, in-phase F3, out-of-phase F4, SPEC/GEO/PALEO/TECTO F5-F8. Analysis: Group statistics F9, Contour F12. Tools: GED Editor, Tensor Addition / Subtraction, Fabric Overprint, Intersection Toolbox, Deform AMS. Settings: Anisotropy Quantitative Factors, Orientation Parameters, Default Colors.

Quirks of the original worth knowing when you drive it: the default view is GEO, so a file without orientation data shows **NGEO = 0 and blank plots until you press S**. Esc closes any tool window. Tool windows open at a different position every time.

## 2. Anisotropy parameters (verified, 1e-7)

With eigenvalues `k1 >= k2 >= k3` normalised to mean 1 and `n_i = ln k_i`, `nm` their mean:

```
L = k1/k2      F = k2/k3      P = k1/k3      E = k2^2 / (k1 k3)
Pj = exp sqrt(2 [(n1-nm)^2 + (n2-nm)^2 + (n3-nm)^2])
T  = (2 n2 - n1 - n3) / (n1 - n3)
U  = (2 k2 - k1 - k3) / (k1 - k3)
Q  = (k1 - k2) / ((k1 + k2)/2 - k3)
```

Undefined (the csv holds -2.0) when `k3 < 0`. K1-K3 and their directions must be recomputed from the raw tensor; the K floats stored in the file are not reliable (see FORMATS.md).

## 3. Group Statistics (verified exactly for N = 12 and N = 60)

- **Mean tensor** = arithmetic mean of the stored tensors (no re-normalisation). Its eigenvalues are the "Normed Semi-Axes" column, directions the Dec/Inc columns, and L F P Pj T U Q E come from it.
- **Average / SEM columns** (really mean and standard deviation, n-1) of Km and of each parameter over the specimens, computed from each specimen's own tensor.
- **Jelinek confidence ellipse** of principal axis `i` (the other two axes are `j`, `k`): rotate every specimen tensor into the principal frame of the mean tensor, take the tilt components `a = t_ij / (tau_i - tau_j)` and `b = t_ik / (tau_i - tau_k)`, form the sample covariance matrix (divisor n-1) of `(a, b)`, and the ellipse has `tan(semi-axis) = sqrt(eigenvalue)`. The two printed numbers per axis are the two semi-axes in degrees (largest first). This is why `K1`'s and `K3`'s shared "e13" are not identical: the ellipse is rotated, its axes are not the plain pairwise angles. Found by testing the pairwise formula first (failed), then the covariance ellipse, then fitting the one constant (it came out as sqrt(N), i.e. no F-distribution factor at all).
- **Bootstrap**: the original prints 90.0 for most values on the test data and draws a cloud of resampled axes. The values could not be reproduced; this program resamples 500 times and uses the 95 % region of the cloud.
- The stereonet draws data points grey (squares K1, triangles K2, circles K3), mean axes as large coloured markers with black outline, Jelinek ellipses as thick coloured outlines, optional thin great circles (K2-K3, K1-K3, K1-K2 planes) in the K colours, bootstrap points as small coloured dots. Table backdrops: K1 pink, K2 light green, K3 lavender, shape parameters cyan.
- It analyses all **active** records; the selection only highlights rows.

## 4. Contour Plot

- Inputs: K1 / K2 / K3 (backdrops pink / green / lavender), Foliation Pole and Lineation (need `.ged` data). Levels: Continuous or Discrete. Smoothing (default 64). Show: Contours, Data Points, Eigenvectors. Buttons: Calculate, Copy Data, Copy Graphics, Close. Caption `N = 60`, `Smoothing = 64`, colour bar with `Max`.
- Colour: white to the colour of the chosen axis (K1 red, K2 green, K3 blue), linear in density/Max. Discrete mode shows bands; bar ticks `0 1 3 5 7 ...` then Max (step 2 for maxima 18.7 and 27.1; other steps are a guess).
- **Eigenvector table** = eigen-decomposition of `sum v v^T / N` of the chosen axes (verified to 5 digits: for one test set 0.85277 / 0.14724 / 0.00000 with dec/inc 178.3/0, 268.3/0, 84.2/90). Eigenvectors are drawn as great circles perpendicular to each eigenvector (E1 red, E2 green, E3 blue, labelled 1 2 3).
- **Density** (approximate): `exp(k (cos^2 t - 1))` with k = the smoothing value, normalised so uniform = 1 (% per 1 % of area). Original maxima 27.1 and 18.7 for two test sets, this gives 26.4 and 18.8.

## 5. Tensor Addition / Subtraction (verified, bit-identical on 86 pairs)

Open set A and set B (`.ams`); columns `#, Name, Field, Freq, Kmean`; per set: Open, Data Filter (Field, Frequency), Disregard, Find Duplicates, Restore All. Pair Records pairs by identical specimen name (unpaired dropped). Combined set C: `C#, Name (A's), A#, B#, Kmean`. For each phase (in-phase and out-of-phase separately):

```
add:       Kc = Ka + Kb        Tc = (|Ka| Ta + |Kb| Tb) / |Kc|
subtract:  Kc = Ka - Kb        Tc = (|Ka| Ta - |Kb| Tb) / |Kc|      (Kc == 0 -> tensor all zeros)
```

(`Ta`, `Tb` are the stored normalised tensors; Km values may be negative for out-of-phase data, hence the absolute values.) Save As writes C as `.ams`: A's record with Km and tensor of both blocks replaced.

## 6. Fabric Overprint (verified, 1e-7)

Inputs per fabric: Kmean, P, T, Kmax Dec/Inc, Kmin Dec/Inc; increments 11 / 101 / 1001 / 10001. Eigenvalues: `ln k1 = lnP/2`, `ln k3 = -lnP/2`, `ln k2 = T lnP/2`, scaled to mean 1; tensor `= Kmean (k1 e1 e1' + k2 e2 e2' + k3 e3 e3')`, `e1 = Kmax`, `e3 = Kmin`, `e2 = e3 x e1`. Calculate mixes linearly, `C(f) = (1-f) A + f B`, `f = i/(n-1)`, on the absolute tensors, and **replaces the main window's data** with specimens named `0 .. n-1` (instrument text `SimulDat`, Km = trace/3, stored tensor normalised).

## 7. Intersection Toolbox (verified exactly)

2 Lineations (trend/plunge) -> plane through both (dip direction/dip) and the angle between them (also shown as its supplement in brackets). 2 Foliations (dip direction/dip) -> intersection lineation (trend/plunge) and the angle between the planes. Examples: lineations 30/40 and 120/20 -> plane 53.4/42.4, angle 77.3 (102.7); foliations 100/30 and 200/60 -> lineation 127.2/27.2, angle 69.0 (111.0). Plot: lineation A red, B blue, result plane green; for foliations A red, B blue, intersection green dot.

## 8. Deform AMS (verified ~1e-6 on 7 cases)

- **Grains**: 316 axes from a cell-centred 20 x 20 square lattice in the equal-area projection (points with r <= 1), inverse-projected into the vector order above (index 0 = plot x, index 1 = plot y), equal weights (found by searching lattice geometries until the resultant fabric of one deformation matched to 1e-6).
- Oblate grains (Tgrain 1): `k1 = k2 = a`, `k3 = a/P`, `a = 3/(2 + 1/P)`; the symmetry axis is the pole and transforms as `F^-T p`. Prolate (Tgrain -1): `k1 = cP`, `k2 = k3 = c`, `c = 3/(2 + P)`; the axis transforms as `F l`. Tgrain is display only.
- Fabric = mean over grains of `base I + (k_axis - base) q q'` (q = transformed unit axis); its eigen-decomposition gives Kmax/Kint/Kmin and L F P Pj T U Q E.
- **Add Step** multiplies: `F_total = F_step F_total` (order matters, verified). Reset Isotropic restores the lattice. Changing the grain type or Pgrain resets.
- **Strain matrices** (s = Stretch, g = Gamma). Uniaxial shortening along k: `1/s` on k. Uniaxial extension: `s` on k. Axially symmetric shortening: `s^(-2/3)` on k and `s^(1/3)` on the others; extension: `s^(2/3)` and `s^(-1/3)`. Pure shear "Shorten A / Extend B" (X/Z, X/Y, Y/X, Y/Z, Z/X, Z/Y): `1/sqrt(s)` on A, `sqrt(s)` on B, 1 on the third. Simple shear (g only): strike-slip XZ/dir X `M[0][1] = g`; strike-slip YZ/dir Y `M[1][0] = g`; thrust in X (XY/dir X) `M[0][2] = g`; thrust in Y (XY/dir Y) `M[1][2] = g`. Transpression (YZ/dir Y): `diag(1/sqrt(s), 1, sqrt(s))` with `M[1][0] = g (sqrt(s) - 1) / ln sqrt(s)`. Each strain type has the axis options listed in `tools.rs`.
- The original starts from the fabric of the data open in the main window; this program starts from the isotropic lattice.

## 9. GED Editor and per-specimen orientation

- Window: Site (name, latitude, longitude), Rock (type, strat, litho, region), Orientation Parameters OP1-OP4 (OP1, OP3 in {3, 6, 9, 12}; OP2, OP4 in {0, 90}; defaults 6, 0, 6, 0), a table `Specimen | Sampling Angles Dec Inc | Foliation #1 Code Dec Dip | Lineation #1 Code Trend Plunge | Foliation #2 ... | Lineation #2 ...`; buttons Open, Clear Table, Add Row, Delete Row(s), Restore Row(s), Save As, Close. File format: see FORMATS.md.
- **Edit Data** (main window) opens a `.ged`, pairs it with the records by name (Full Match / Match from Left / Continue), and writes the orientation into the **records of the `.ams`** (File > Save Active Records As makes it permanent). Geographic coordinates are computed from these per-specimen values, not from global settings.
- **Geographic rule (verified for OP1, OP2, OP3 on two specimens)**: `R(spec -> geo) = Rz((6 - OP3) 30 deg) * [eX eY eZ] * Rz((OP1 - 6) 30 deg)` where `eX` points to azimuth `alpha` with plunge `phi'`, `eY` is horizontal at azimuth `alpha + 90`, `eZ = eX x eY`, and `phi' = phi` for OP2 = 0, `90 - phi` for OP2 = 90 (which also reverses `eY`). OP4 (foliation convention) not isolated.
- **Paleo (P)**: the geographic data tilted so that the foliation carrying **code 7** becomes horizontal (verified for two specimens). **Tectonic (T)**: P plus a further rotation about the vertical (-11 deg for one specimen, +82 for another); rule not decoded.
- This program does **not** implement the per-specimen route yet: its G/P/T views use the global values in Settings > Orientation Parameters, which are an approximation.

## 10. Other behaviours seen

- Esc closes the tool windows. Group Statistics and Contour use all active records.
- Data Table columns: `# Specimen Field Freq Kmean Kmax(Dec Inc) Kint(Dec Inc) Kmin(Dec Inc) L F P Pj T U Q E F ...` (the F-test columns continue off screen), coloured headers (Kmean pink, Kmax red, Kint green, Kmin lavender, shape cyan).
- Selecting does not filter group statistics; "Disregard" does.
