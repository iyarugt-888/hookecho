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
    cache: crate::ui::region_stats_window::RegionCache,
    /// Gates a hovered chart bar or scatter cell stands for, and the pass that set them: they draw
    /// only while something keeps hovering.
    hl: Vec<HighlightGate>,
    hl_pass: u64,
}

/// Most gates the map outlines for one hovered bar; past it every n-th stands in.
const MAX_HIGHLIGHT: usize = 20_000;

/// A gate a hovered chart stands for: where it is over the ground, and where on its beam, so the
/// 3D view can put it on its own tilt rather than on the ground under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HighlightGate {
    pub ll: [f64; 2],
    pub az_deg: f32,
    /// Slant range, km.
    pub range_km: f32,
    pub elevation_deg: f32,
}

/// The gates of `s` that `h` keeps.
pub(crate) fn highlight_points(
    s: &RegionSamples,
    h: &crate::ui::region_stats_window::Highlight,
) -> Vec<HighlightGate> {
    let pts: Vec<HighlightGate> = s
        .rows
        .iter()
        .filter(|r| h.matches(r))
        .map(|r| HighlightGate {
            ll: [r.lon, r.lat],
            az_deg: r.az_deg,
            range_km: r.range_km,
            elevation_deg: r.elevation_deg,
        })
        .collect();
    let step = pts.len().div_ceil(MAX_HIGHLIGHT).max(1);
    pts.into_iter().step_by(step).collect()
}

impl RegionStatsState {
    /// The gathered box, while its window is open.
    pub(crate) fn samples(&self) -> Option<&RegionSamples> {
        self.samples.as_ref()
    }

    /// The samples and the window's choices together, for the workstation's Region window.
    pub(crate) fn dock_parts(
        &mut self,
    ) -> Option<(
        &RegionSamples,
        &mut crate::ui::region_stats_window::RegionCache,
        &mut crate::ui::region_stats_window::RegionStatsUi,
    )> {
        match (&self.samples, &mut self.cache, &mut self.ui) {
            (Some(s), cache, ui) => Some((s, cache, ui)),
            _ => None,
        }
    }

    /// Outline `pts` on the map this pass (and the next, whichever of the map and the window
    /// draws first).
    pub(crate) fn set_highlight(&mut self, pass: u64, pts: Vec<HighlightGate>) {
        self.hl = pts;
        self.hl_pass = pass;
    }

    /// The window's choices, for its "Use as 3D region" request.
    pub(crate) fn ui_mut(&mut self) -> &mut crate::ui::region_stats_window::RegionStatsUi {
        &mut self.ui
    }

    /// Close the box: its samples, its corners and any outline.
    pub(crate) fn clear(&mut self) {
        self.samples = None;
        self.pts.clear();
        self.hl.clear();
    }

    /// Outline the box being drawn: a dot for the first corner, the rectangle once there are two.
    /// `screen` maps `[lon, lat]` to the map's screen position, `gate` a hovered gate to where the
    /// pane draws it (on its tilt in 3D), `None` when it is not on screen.
    pub(crate) fn paint(
        &self,
        painter: &egui::Painter,
        screen: impl Fn([f64; 2]) -> egui::Pos2,
        gate: impl Fn(&HighlightGate) -> Option<egui::Pos2>,
    ) {
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
        let pass = painter.ctx().cumulative_pass_nr();
        if !self.hl.is_empty() && self.hl_pass + 1 >= pass {
            for p in self.hl.iter().filter_map(&gate) {
                painter.circle_filled(p, 1.3, egui::Color32::WHITE);
            }
            // One more pass takes the outline away once nothing hovers.
            painter.ctx().request_repaint();
        }
    }
}

impl HookEchoApp {
    /// Where pane `idx` draws hovered gate `g`, in screen points within `prect`. On the map, over
    /// the ground. In 3D, where the volume draws that gate: on its tilt's beam among the observed
    /// sweeps (beam rise and exaggeration as drawn, like the 3D ruler's ends), or at its height
    /// above sea level in a resampled volume. `None` behind the camera.
    pub(crate) fn highlight_screen(
        &self,
        idx: usize,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
        prect: egui::Rect,
        g: &HighlightGate,
    ) -> Option<egui::Pos2> {
        let at = |p: (f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
        let v = &self.views[idx];
        let site = v.site.as_deref().and_then(wxdata::sites::site_by_id);
        let (true, Some(site)) = (v.map_3d.enabled, site) else {
            let w = crate::render::mercator::lonlat_to_world(g.ll[0], g.ll[1]);
            return Some(at(cam.world_to_screen(w, vp)));
        };
        let antenna_m = f64::from(site.elevation_meters) + wxdata::towers::tower_m(site.id);
        let (slant, elev) = (f64::from(g.range_km), f64::from(g.elevation_deg));
        let vex = f64::from(v.map_3d.vertical_exaggeration);
        if v.map_3d.representation == crate::view::Map3dRepresentation::ObservedSweeps {
            let local = crate::render3d::beam_local(
                cam,
                f64::from(site.longitude),
                f64::from(site.latitude),
                antenna_m,
                vex,
                f64::from(v.map_3d.beam_rise),
                f64::from(g.az_deg),
                wxdata::xsection::ground_from_slant_km(slant, elev),
                g.elevation_deg,
            );
            return crate::render3d::project_local(cam, vp, local).map(at);
        }
        let km = antenna_m / 1000.0 + wxdata::xsection::beam_height_km(slant, elev);
        crate::render3d::lonlat_alt_screen(cam, vp, g.ll[0], g.ll[1], km, vex).map(at)
    }

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
        // The workstation shows these in its Region tool window (`chrome/dock/region.rs`).
        if self.workstation_chrome() {
            return;
        }
        let Some(s) = &self.region.samples else {
            return;
        };
        if !crate::ui::region_stats_window::show(ctx, s, &mut self.region.ui, &mut self.drawer) {
            self.region.clear();
        }
        self.take_region_box_roi();
    }
}
