//! A finite marker of the latest data received, with no predicted angular motion.

use crate::view::MapView;
use std::sync::Arc;
use wxdata::clock::Instant;
use wxdata::level2::Scan;

pub(crate) struct LiveSweep {
    scan: crate::volume::ScanIdentity,
    site: Option<String>,
    received: Instant,
    observation: wxdata::live_beam::Observation,
}

impl LiveSweep {
    pub(crate) fn capture(
        scan: &Arc<Scan>,
        site: Option<&str>,
        coverage: &wxdata::live::RadialCoverage,
    ) -> Option<Self> {
        Some(Self {
            scan: crate::volume::ScanIdentity::new(scan),
            site: site.map(str::to_owned),
            received: Instant::now(),
            observation: wxdata::live_beam::observe(scan, coverage)?,
        })
    }

    fn frame(&self, view: &MapView) -> Option<Frame> {
        let volume = view.volume.as_ref()?;
        let elevation = *volume.elevations.get(view.tilt)?;
        if view.live_progress.is_none()
            || !volume.is_live_partial()
            || !self.scan.matches(&volume.scan)
            || self.site != view.site
            || (elevation - self.observation.elevation_deg).abs() >= 0.05
        {
            return None;
        }
        let edge = self.observation.edges[view.moment.index()]?;
        fade(edge, self.received.elapsed().as_secs_f32())
    }
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    edge: wxdata::live_beam::Edge,
    alpha: f32,
    remaining_secs: f32,
}

fn fade(edge: wxdata::live_beam::Edge, elapsed: f32) -> Option<Frame> {
    const HOLD: f32 = 1.0;
    const FADE: f32 = 0.35;
    if !elapsed.is_finite() || elapsed < 0.0 || elapsed >= HOLD + FADE {
        return None;
    }
    Some(Frame {
        edge,
        alpha: (1.0 - (elapsed - HOLD).max(0.0) / FADE).clamp(0.0, 1.0),
        remaining_secs: HOLD + FADE - elapsed,
    })
}

/// Each point is a destination at a finite native gate range, not a ray to the screen edge.
fn points(
    rect: egui::Rect,
    camera: crate::render::mercator::Camera,
    vp: (f32, f32),
    radar: [f64; 2],
    edge: wxdata::live_beam::Edge,
) -> Vec<egui::Pos2> {
    (0..=16)
        .map(|step| {
            let point = crate::geo::destination_point(
                radar,
                f64::from(edge.bearing_deg),
                f64::from(edge.range_km) * f64::from(step) / 16.0,
            );
            let world = crate::render::mercator::lonlat_to_world(point[0], point[1]);
            let (x, y) = camera.world_to_screen(world, vp);
            egui::pos2(rect.left() + x, rect.top() + y)
        })
        .collect()
}

fn paint(
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: crate::render::mercator::Camera,
    vp: (f32, f32),
    radar: [f64; 2],
    frame: Frame,
) {
    let points = points(rect, camera, vp, radar, frame.edge);
    let green = egui::Color32::from_rgb(82, 255, 116).gamma_multiply(frame.alpha);
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(5.0, egui::Color32::BLACK.gamma_multiply(frame.alpha * 0.72)),
    ));
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(2.25, green),
    ));
    if let Some(endpoint) = points.last() {
        painter.circle_filled(*endpoint, 3.0, green);
    }
}

pub(super) fn draw(
    view: &MapView,
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: crate::render::mercator::Camera,
    vp: (f32, f32),
    ctx: &egui::Context,
) {
    if view.map_3d.enabled
        || view.user_product.is_some()
        || view.column_product.is_some()
        || !view.show_radar
        || !view.timeline.following
        || view.timeline.playing
    {
        return;
    }
    let Some(frame) = view.live_sweep.as_ref().and_then(|sweep| sweep.frame(view)) else {
        return;
    };
    let Some(site) = view.site.as_deref().and_then(wxdata::sites::site_by_id) else {
        return;
    };
    paint(
        painter,
        rect,
        camera,
        vp,
        [site.longitude as f64, site.latitude as f64],
        frame,
    );
    if crate::ui::motion::reduced() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(frame.remaining_secs));
    } else {
        ctx.request_repaint(); // fade only; no motion beyond received data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::live::{CutKind, RadialCoverage, ScanProgress};

    fn received_view() -> MapView {
        let scan = Arc::new(
            wxdata::level2::decode_volume(
                include_bytes!(
                    "../../../wxdata/tests/data/corpus/denver-hail-2017-first-records.ar2"
                )
                .to_vec(),
            )
            .unwrap(),
        );
        let sweep = &scan.sweeps()[0];
        let progress = ScanProgress {
            volume_start_ms: None,
            vcp_number: Some(scan.coverage_pattern_number().number()),
            cut_kind: CutKind::Standard,
            elevation_number: usize::from(sweep.elevation_number()),
            total_elevations: scan.sweeps().len(),
            elevation_angle_deg: f64::from(sweep.elevation_angle_degrees().unwrap()),
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 60.0,
            chunk_index: 1,
            chunks_in_sweep: 6,
        };
        let coverage = RadialCoverage {
            progress,
            radials: sweep
                .radials()
                .iter()
                .map(|r| (r.azimuth_number(), r.collection_timestamp()))
                .collect(),
            source_passes: None,
            source_sequences: None,
            source_attribution: None,
            source_scope: None,
        };
        let mut view = MapView::new(
            Some("KFTG".into()),
            crate::render::mercator::Camera::at_lonlat(-104.5, 39.8, 5.0),
        );
        view.live_sweep = LiveSweep::capture(&scan, view.site.as_deref(), &coverage);
        view.live_progress = Some(progress);
        view.volume = Some(crate::view::Volume::from_live(
            scan,
            "fixture".into(),
            chrono::Utc::now(),
        ));
        view
    }

    #[test]
    fn the_marker_is_owned_by_the_displayed_scan_site_tilt_and_moment() {
        let mut view = received_view();
        let frame = view.live_sweep.as_ref().unwrap().frame(&view).unwrap();
        assert!(frame.edge.range_km > 0.0 && frame.edge.range_km < 500.0);
        view.site = Some("KTLX".into());
        assert!(view.live_sweep.as_ref().unwrap().frame(&view).is_none());
        view.site = Some("KFTG".into());
        view.tilt = usize::MAX;
        assert!(view.live_sweep.as_ref().unwrap().frame(&view).is_none());
        view.tilt = 0;
        view.live_progress = None; // ended subscription
        assert!(view.live_sweep.as_ref().unwrap().frame(&view).is_none());
        view.live_progress = Some(ScanProgress {
            elevation_angle_deg: 0.5,
            ..received_view().live_progress.unwrap()
        });
        let old = view.volume.take().unwrap();
        // Equal name/data are not the same accepted revision/allocation.
        view.volume = Some(crate::view::Volume::from_live(
            Arc::new((*old.scan).clone()),
            old.name,
            old.time,
        ));
        assert!(view.live_sweep.as_ref().unwrap().frame(&view).is_none());
    }

    #[test]
    fn waiting_for_data_never_rotates_the_marker_and_a_stall_expires_it() {
        let edge = wxdata::live_beam::Edge {
            bearing_deg: 359.75,
            range_km: 200.0,
            acquired_ms: 100,
        };
        for elapsed in [0.0, 0.5, 1.2] {
            assert_eq!(fade(edge, elapsed).unwrap().edge, edge);
        }
        assert!(fade(edge, 1.36).is_none());
        assert!(fade(edge, f32::NAN).is_none());
        assert!(fade(edge, -1.0).is_none());
    }

    #[test]
    fn conus_view_ends_at_the_native_range_instead_of_the_pane_boundary() {
        let radar = [-97.0, 35.0];
        let camera = crate::render::mercator::Camera::at_lonlat(radar[0], radar[1], 3.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0));
        let edge = wxdata::live_beam::Edge {
            bearing_deg: 90.0,
            range_km: 200.0,
            acquired_ms: 100,
        };
        let line = points(rect, camera, (rect.width(), rect.height()), radar, edge);
        assert_eq!(line.len(), 17);
        assert!(line.iter().all(|p| p.is_finite() && rect.contains(*p)));
        assert!(line[0].distance(*line.last().unwrap()) < rect.width() * 0.1);
        let endpoint = crate::geo::destination_point(radar, 90.0, 200.0);
        let world = crate::render::mercator::lonlat_to_world(endpoint[0], endpoint[1]);
        let (x, y) = camera.world_to_screen(world, (rect.width(), rect.height()));
        assert_eq!(*line.last().unwrap(), egui::pos2(x, y));
    }

    #[test]
    #[ignore = "GPU capture"]
    fn capture_finite_received_beam_at_two_map_scales() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        let dir = std::path::Path::new("target/parity-review/m1.1/beam");
        std::fs::create_dir_all(dir).unwrap();
        for (name, zoom) in [("conus", 3.0), ("local", 6.0)] {
            gpu.save(&dir.join(format!("{name}.png")), 960, 600, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                let camera = crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, zoom);
                let edge = wxdata::live_beam::Edge {
                    bearing_deg: 60.0,
                    range_km: 200.0,
                    acquired_ms: 100,
                };
                paint(
                    &painter,
                    rect,
                    camera,
                    (rect.width(), rect.height()),
                    [-97.0, 35.0],
                    fade(edge, 0.0).unwrap(),
                );
                painter.text(
                    rect.left_top() + egui::vec2(20.0, 20.0),
                    egui::Align2::LEFT_TOP,
                    "Received radial: 60° • 200 km endpoint • no antenna prediction",
                    egui::FontId::proportional(18.0),
                    egui::Color32::WHITE,
                );
            })
            .unwrap();
        }
    }
}
