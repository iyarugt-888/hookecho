//! The region-statistics tool (ROADMAP_NEW C4): two clicks on the map set opposite corners of a
//! box, and every gate of the displayed tilt inside it is gathered across every moment for
//! [`crate::ui::region_stats_window`] — the summary table, histogram, scatter plot and CSV.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1): the app keeps one field and a
//! handful of one-line calls into here.

use super::HookEchoApp;
use wxdata::level2::Moment;
use wxdata::regionstats::RegionSamples;

/// The tool's state: the corners clicked so far, the gathered samples, and the window's choices.
#[derive(Default)]
pub(crate) struct RegionStatsState {
    /// `[lon, lat]` corners, at most two; a third click starts a new box.
    pts: Vec<[f64; 2]>,
    samples: Option<RegionSamples>,
    ui: crate::ui::region_stats_window::RegionStatsUi,
}

impl RegionStatsState {
    /// The gathered box, while its window is open.
    pub(crate) fn samples(&self) -> Option<&RegionSamples> {
        self.samples.as_ref()
    }

    /// Outline the box being drawn: a dot for the first corner, the rectangle once there are two.
    /// `screen` maps `[lon, lat]` to the map's screen position.
    pub(crate) fn paint(&self, painter: &egui::Painter, screen: impl Fn([f64; 2]) -> egui::Pos2) {
        // Orange: apart from the yellow measure line and the cyan cross-section.
        let col = egui::Color32::from_rgb(255, 150, 60);
        match self.pts.as_slice() {
            [a] => {
                painter.circle_filled(screen(*a), 3.5, col);
            }
            [a, b] => {
                let rect = egui::Rect::from_two_pos(screen(*a), screen(*b));
                painter.rect_filled(rect, 0.0, col.gamma_multiply(0.08));
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0, col),
                    egui::StrokeKind::Middle,
                );
            }
            _ => {}
        }
    }
}

impl HookEchoApp {
    /// A click with the region tool: the first corner, then the second (which gathers the box),
    /// then a fresh box.
    pub(crate) fn region_click(&mut self, idx: usize, lon: f64, lat: f64) {
        if self.region.pts.len() >= 2 {
            self.region.pts.clear();
        }
        self.region.pts.push([lon, lat]);
        if self.region.pts.len() == 2 {
            self.build_region_stats(idx);
        }
    }

    /// Gather every gate of pane `idx`'s displayed tilt inside the box, reflectivity first (it
    /// reaches furthest, so its gates are the rows) and velocity dealiased as everywhere else.
    fn build_region_stats(&mut self, idx: usize) {
        let [a, b] = [self.region.pts[0], self.region.pts[1]];
        let bbox = wxdata::regionstats::bbox_of(a, b);
        let view = &mut self.views[idx];
        let tilt = view.tilt;
        let Some(vol) = view.volume.as_mut() else {
            return;
        };
        let sweeps: Vec<_> = Moment::ALL
            .into_iter()
            .filter_map(|m| vol.binned(m, tilt, m == Moment::Velocity).ok().cloned())
            .collect();
        let fresh = self.region.samples.is_none();
        self.region.samples = wxdata::regionstats::gather(&sweeps, bbox);
        match &self.region.samples {
            // A first box opens on the pair the roadmap names first; later boxes keep the choice.
            Some(s) if fresh => {
                if let Some(zdr) = s.index_of(Moment::DifferentialReflectivity) {
                    self.region.ui.y = zdr;
                }
            }
            Some(_) => {}
            None => self.banner(
                "Region statistics".to_string(),
                "No radar data inside that box on this tilt.".to_string(),
            ),
        }
    }

    /// The window, while there are samples to show; closing it clears the box.
    pub(crate) fn show_region_stats(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.region.samples else {
            return;
        };
        if !crate::ui::region_stats_window::show(ctx, s, &mut self.region.ui, &mut self.drawer) {
            self.region.samples = None;
            self.region.pts.clear();
        }
    }
}
