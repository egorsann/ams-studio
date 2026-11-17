//! What the three plot panes (top right, bottom left, bottom right) draw: every one of them can be any plot type
//! with any X / Y parameter. The settings are kept in `plots.txt` next to `state.txt`, so they survive a restart,
//! and a pane can be made *specimen specific*: its settings are then kept for the open file only, and any other
//! file shows the general settings again.

use super::{config_dir, App};
use crate::plots::{AxisCfg, GroupBy, XyKind};
use std::path::{Path, PathBuf};

/// One pane's settings.
#[derive(Clone, PartialEq)]
pub(super) struct PaneCfg {
    pub kind: XyKind,
    /// The parameter of a histogram.
    pub hist: AxisCfg,
    pub x: AxisCfg,
    pub y: AxisCfg,
    /// Histogram bins, or the classes of X of a bar / box plot.
    pub bins: usize,
    /// Only the sub-specimens that are selected in the specimen list.
    pub sel_only: bool,
    pub diag: bool,
    pub conf: bool,
    pub connect: bool,
    pub legend: bool,
    pub zero: bool,
    /// How a bar or box plot forms its groups.
    pub group: GroupBy,
    /// The bottom right pane (T vs Pj) as a polar plot (T as the angle, Pj as the radius) instead of the plot type above.
    pub polar: bool,
    /// The radius of the polar plot is logarithmic.
    pub polar_log: bool,
}

impl PaneCfg {
    /// The original Anisoft 6 plot of pane `k` (0 top right: N vs Kmean, 1 bottom left: P vs Kmean, 2 bottom right: T vs P);
    /// P is Pj unless `use_p`.
    pub fn default_for(k: usize, use_p: bool) -> PaneCfg {
        let p = if use_p { 3 } else { 4 };
        let mut c = PaneCfg {
            kind: if k == 0 { XyKind::Histogram } else { XyKind::Scatter },
            hist: AxisCfg::new(0),
            x: AxisCfg::new(0),
            y: AxisCfg::new(p),
            bins: 12,
            sel_only: false,
            diag: false,
            conf: false,
            connect: false,
            legend: true,
            zero: false,
            group: GroupBy::Classes,
            polar: false,
            polar_log: false,
        };
        if k == 2 {
            c.x = AxisCfg::new(p);
            c.y = AxisCfg::new(5);
            c.y.auto = false;
            c.y.min = -1.0;
            c.y.max = 1.0;
            c.zero = true;
        }
        c
    }

    fn to_line(&self) -> String {
        let a = |a: &AxisCfg| format!("{}:{}:{}:{}:{}", a.param, a.auto as u8, a.min, a.max, a.log as u8);
        let kind = XyKind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0);
        format!(
            "{kind}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            a(&self.hist), a(&self.x), a(&self.y), self.bins, self.sel_only as u8, self.diag as u8, self.conf as u8, self.connect as u8, self.legend as u8, self.zero as u8, (self.group == GroupBy::Sample) as u8, self.polar as u8, self.polar_log as u8
        )
    }

    fn from_line(s: &str) -> Option<PaneCfg> {
        let f: Vec<&str> = s.trim().split('|').collect();
        // 11 fields: written before the group-by choice existed; 12: before the polar plot
        if f.len() != 11 && f.len() != 12 && f.len() != 14 {
            return None;
        }
        let axis = |t: &str| -> Option<AxisCfg> {
            let v: Vec<&str> = t.split(':').collect();
            if v.len() != 5 {
                return None;
            }
            let mut a = AxisCfg::new(v[0].parse::<usize>().ok()?.min(ams_core::params::N_PARAMS - 1));
            a.auto = v[1] == "1";
            a.min = v[2].parse().ok()?;
            a.max = v[3].parse().ok()?;
            a.log = v[4] == "1";
            Some(a)
        };
        let flag = |t: &str| t == "1";
        Some(PaneCfg {
            kind: *XyKind::ALL.get(f[0].parse::<usize>().ok()?)?,
            hist: axis(f[1])?,
            x: axis(f[2])?,
            y: axis(f[3])?,
            bins: f[4].parse::<usize>().ok()?.clamp(2, 60),
            sel_only: flag(f[5]),
            diag: flag(f[6]),
            conf: flag(f[7]),
            connect: flag(f[8]),
            legend: flag(f[9]),
            zero: flag(f[10]),
            group: if f.get(11).map_or(false, |t| flag(t)) { GroupBy::Sample } else { GroupBy::Classes },
            polar: f.get(12).map_or(false, |t| flag(t)),
            polar_log: f.get(13).map_or(false, |t| flag(t)),
        })
    }
}

/// How long *Undo reset* stays available.
pub(super) const RESET_UNDO_SECS: u64 = 600;

/// Everything *Reset all panes* changes, kept for the undo.
pub(super) struct ResetSnap {
    pub at: std::time::Instant,
    panes: [PaneCfg; 3],
    key: Option<PathBuf>,
    spec: [Option<PaneCfg>; 3],
    show_k: [bool; 3],
    st: (bool, bool, bool, bool, bool),
    overlay: super::analysis_ui::StereoOverlay,
    color: [crate::plots::ColorCfg; 4],
    color_own: [bool; 4],
}

fn plots_path() -> PathBuf {
    config_dir().join("ams-studio").join("plots.txt")
}

impl App {
    /// The file the specimen-specific settings belong to (the open file; a data set without a file has none).
    fn spec_key(&self) -> Option<PathBuf> {
        self.doc.as_ref().filter(|d| !d.synthetic).map(|d| d.path.clone())
    }

    /// The settings of pane `k` that apply to the open file: its own when it is specimen specific, else the general ones.
    pub(super) fn cfg(&self, k: usize) -> &PaneCfg {
        let key = self.doc.as_ref().filter(|d| !d.synthetic).map(|d| &d.path);
        key.and_then(|p| self.spec_cfg[k].get(p)).unwrap_or(&self.panes[k])
    }

    pub(super) fn cfg_mut(&mut self, k: usize) -> &mut PaneCfg {
        if let Some(key) = self.spec_key().filter(|p| self.spec_cfg[k].contains_key(p)) {
            if let Some(c) = self.spec_cfg[k].get_mut(&key) {
                return c;
            }
        }
        &mut self.panes[k]
    }

    pub(super) fn is_specific(&self, k: usize) -> bool {
        self.spec_key().map_or(false, |p| self.spec_cfg[k].contains_key(&p))
    }

    /// Makes pane `k` specimen specific for the open file (starting from what it shows now) or general again.
    pub(super) fn set_specific(&mut self, k: usize, on: bool) {
        let Some(key) = self.spec_key() else { return };
        if on {
            let now = self.cfg(k).clone();
            self.spec_cfg[k].insert(key, now);
        } else {
            self.spec_cfg[k].remove(&key);
        }
        self.views[k].reset();
        self.save_plots();
    }

    /// View > Reset all panes to default: the four panes go back to their Anisoft 6 settings (the stereonet options, the
    /// three plots, their own colour-coding, the zoom). The open file's specimen-specific settings are dropped too.
    pub(super) fn reset_all_panes(&mut self) {
        let key = self.spec_key();
        let snap = ResetSnap {
            at: std::time::Instant::now(),
            panes: self.panes.clone(),
            key: key.clone(),
            spec: [0, 1, 2].map(|k| key.as_ref().and_then(|p| self.spec_cfg[k].get(p).cloned())),
            show_k: self.show_k,
            st: (self.st_data, self.st_caption, self.st_legend, self.st_connect, self.st_mf),
            overlay: self.st_overlay.clone(),
            color: self.color.clone(),
            color_own: self.color_own,
        };
        self.panes = [PaneCfg::default_for(0, self.use_p), PaneCfg::default_for(1, self.use_p), PaneCfg::default_for(2, self.use_p)];
        if let Some(p) = &key {
            self.spec_cfg.iter_mut().for_each(|m| {
                m.remove(p);
            });
        }
        self.show_k = [true; 3];
        (self.st_data, self.st_caption, self.st_legend, self.st_connect, self.st_mf) = (true, true, true, false, false);
        self.st_overlay = Default::default();
        self.color_own = [false; 4];
        self.apply_master_color();
        self.st_view.reset();
        self.views.iter_mut().for_each(|v| v.reset());
        self.pane_settings = [false; 4];
        self.save_plots();
        self.reset_snap = Some(snap);
    }

    /// Takes the last reset back (only offered for ten minutes).
    pub(super) fn undo_reset_panes(&mut self) {
        let Some(s) = self.reset_snap.take() else { return };
        if s.at.elapsed().as_secs() >= RESET_UNDO_SECS {
            return;
        }
        self.panes = s.panes;
        if let Some(p) = &s.key {
            for (k, c) in s.spec.into_iter().enumerate() {
                if let Some(c) = c {
                    self.spec_cfg[k].insert(p.clone(), c);
                }
            }
        }
        self.show_k = s.show_k;
        (self.st_data, self.st_caption, self.st_legend, self.st_connect, self.st_mf) = s.st;
        self.st_overlay = s.overlay;
        self.color = s.color;
        self.color_own = s.color_own;
        self.save_plots();
    }

    /// Minutes left for the undo of the last reset (None when there is nothing to undo or the time is over).
    pub(super) fn reset_undo_left(&self) -> Option<u64> {
        self.reset_snap.as_ref().map(|s| RESET_UNDO_SECS.saturating_sub(s.at.elapsed().as_secs())).filter(|r| *r > 0)
    }

    /// The settings of the plot panes as the text of `plots.txt`.
    pub(super) fn plots_text(&self) -> String {
        let mut s = String::new();
        for (k, c) in self.panes.iter().enumerate() {
            s += &format!("pane{k}={}\n", c.to_line());
        }
        for (k, m) in self.spec_cfg.iter().enumerate() {
            let mut v: Vec<(&PathBuf, &PaneCfg)> = m.iter().collect();
            v.sort_by(|a, b| a.0.cmp(b.0));
            for (path, c) in v {
                s += &format!("spec{k}={}\t{}\n", path.display(), c.to_line());
            }
        }
        s
    }

    pub(super) fn save_plots(&self) {
        let p = plots_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, self.plots_text());
    }

    pub(super) fn load_plots(&mut self) {
        let Ok(text) = std::fs::read_to_string(plots_path()) else { return };
        for l in text.lines() {
            let Some((key, val)) = l.split_once('=') else { continue };
            if let Some(k) = key.strip_prefix("pane").and_then(|k| k.parse::<usize>().ok()).filter(|k| *k < 3) {
                if let Some(c) = PaneCfg::from_line(val) {
                    self.panes[k] = c;
                }
            } else if let Some(k) = key.strip_prefix("spec").and_then(|k| k.parse::<usize>().ok()).filter(|k| *k < 3) {
                if let Some((path, cfg)) = val.rsplit_once('\t') {
                    if let Some(c) = PaneCfg::from_line(cfg) {
                        self.spec_cfg[k].insert(Path::new(path).to_path_buf(), c);
                    }
                }
            }
        }
    }

    /// P (or Pj) in every pane axis that shows one of them follows the choice made in the headers.
    pub(super) fn apply_p_choice(&mut self) {
        let to = if self.use_p { 3 } else { 4 };
        let fix = |c: &mut PaneCfg| {
            for a in [&mut c.x, &mut c.y, &mut c.hist] {
                if a.param == 3 || a.param == 4 {
                    a.param = to;
                }
            }
        };
        self.panes.iter_mut().for_each(fix);
        self.spec_cfg.iter_mut().flat_map(|m| m.values_mut()).for_each(fix);
    }

    /// The titles of the four panes (they follow what the plots show).
    pub(super) fn pane_names(&self) -> [String; 4] {
        let pj = if self.use_p { "P" } else { "Pj" };
        let name = |k: usize| {
            let c = self.cfg(k);
            if k == 2 && c.polar {
                return format!("T vs {pj} (polar)");
            }
            match c.kind {
                XyKind::Histogram => format!("N vs {}", c.hist.label()),
                XyKind::Scatter => format!("{} vs {}", c.y.label(), c.x.label()),
                other if matches!(other, XyKind::Bar | XyKind::Box) && c.group == GroupBy::Sample => format!("{} by sample ({})", c.y.label(), other.short()),
                other => format!("{} vs {} ({})", c.y.label(), c.x.label(), other.short()),
            }
        };
        ["Stereonet".to_string(), name(0), name(1), name(2)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_setting_survives_the_text_form() {
        let mut c = PaneCfg::default_for(2, false);
        c.kind = XyKind::Box;
        c.x.log = true;
        c.y.auto = false;
        c.y.min = -0.25;
        c.bins = 7;
        c.sel_only = true;
        c.polar = true;
        c.polar_log = true;
        let back = PaneCfg::from_line(&c.to_line()).unwrap();
        assert!(back == c);
        assert!(PaneCfg::from_line("garbage").is_none());
    }

    #[test]
    fn the_defaults_are_the_anisoft_plots() {
        let h = PaneCfg::default_for(0, false);
        assert!(h.kind == XyKind::Histogram && h.hist.param == 0);
        let b = PaneCfg::default_for(1, true);
        assert!(b.kind == XyKind::Scatter && b.x.param == 0 && b.y.param == 3);
        let t = PaneCfg::default_for(2, false);
        assert!(t.x.param == 4 && t.y.param == 5 && !t.y.auto && t.zero);
    }
}
