//! Wind particles from a wind picked in "Model fields" (ROADMAP_NEW F4 "wind particles"): the
//! same particle layer the HRRR 10 m wind and radar winds drive, fed from the east/north
//! components the browsed wind's barbs are drawn from, on the active pane. Rebuilt whenever that
//! pane's accepted wind changes (a new lead, run, level or model); kept while the next one loads,
//! dropped once the pane no longer shows a browsed wind.

use super::model_field::WindPair;
use super::HookEchoApp;
use crate::wind_draw::WindField;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct ModelParticles {
    pub(crate) on: bool,
    /// The components the particles were last built from.
    from: Option<Arc<WindPair>>,
}

/// The particle field for a browsed wind's components.
fn particle_field(pair: &WindPair, run: chrono::DateTime<chrono::Utc>, lead_h: u16) -> WindField {
    WindField {
        u: pair.0.clone(),
        v: pair.1.clone(),
        // The particle layer only reads the level for its upload key, which is reset below.
        level: wxdata::hrrr::WindLevel::Surface,
        run,
        fcst_hour: u8::try_from(lead_h).unwrap_or(u8::MAX),
    }
}

impl HookEchoApp {
    /// Whether the active pane shows a browsed wind (accepted or still loading).
    fn active_pane_shows_model_wind(&self) -> bool {
        let v = &self.views[self.active];
        v.fields_on.contains(&crate::render::FieldLayer::ModelField)
            && v.models.field.as_ref().is_some_and(|p| p.vector)
    }

    /// While on, keep the particles on the active pane's browsed wind.
    pub(crate) fn sync_model_particles(&mut self) {
        if !self.model_particles.on || !self.show_wind {
            return;
        }
        if !self.active_pane_shows_model_wind() {
            if self.model_particles.from.take().is_some() {
                self.wind = None;
            }
            return;
        }
        // Still loading: the last wind keeps drifting until the new one is accepted.
        let Some(pair) = self.model_wind_for(self.active) else {
            return;
        };
        if self
            .model_particles
            .from
            .as_ref()
            .is_some_and(|f| Arc::ptr_eq(f, &pair))
        {
            return;
        }
        let (run, lead) = match self
            .selected_model_request_for(self.active, crate::render::FieldLayer::ModelField)
        {
            Some(super::ModelRequest::Discovered(_, lead, run)) => {
                (run.unwrap_or_else(chrono::Utc::now), lead)
            }
            _ => (chrono::Utc::now(), 0),
        };
        self.wind = Some(particle_field(&pair, run, lead));
        // The GPU layer keys its upload on run, lead and level, which two browsed winds can
        // share (10 m and 250 hPa from one run): upload whatever arrives.
        self.wind_uploaded = None;
        self.model_particles.from = Some(pair);
    }

    /// The browsed wind's particles switched on or off: the particles need a field from the
    /// other source, and only one of radar winds and a browsed wind drives them.
    pub(crate) fn model_particles_toggled(&mut self) {
        self.wind = None;
        self.wind_fetched = None;
        self.wind_uploaded = None;
        self.model_particles.from = None;
        if self.model_particles.on {
            self.show_wind = true;
            if self.radar_wind.on {
                self.radar_wind.on = false;
                self.radar_wind_toggled();
            }
        }
    }

    /// Whether the HRRR 10 m wind is the particles' source (neither radar winds nor a browsed
    /// wind has taken them over).
    pub(crate) fn particles_from_hrrr(&self) -> bool {
        !self.radar_wind.on && !self.model_particles.on
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(value: f32) -> wxdata::mrms::MrmsField {
        wxdata::mrms::MrmsField {
            nx: 4,
            ny: 4,
            lat_north: 40.0,
            lat_south: 37.0,
            lon_west: -100.0,
            lon_east: -97.0,
            values: vec![value; 16],
            time: chrono::Utc::now(),
        }
    }

    /// The particles move with the browsed wind's own components, in m/s, at its lead.
    #[test]
    fn particles_carry_the_browsed_components() {
        let run = chrono::Utc::now();
        let f = particle_field(&(grid(12.0), grid(-5.0)), run, 18);
        assert_eq!(f.sample(-98.5, 38.5), Some((12.0, -5.0)));
        assert_eq!(f.valid(), run + chrono::Duration::hours(18));
        assert_eq!(
            f.sample(-120.0, 38.5),
            None,
            "outside the grid there is no wind"
        );
    }

    /// Live (network, GPU): the newest HRRR run's 850 hPa wind, fetched as a browsed wind is
    /// (`fetch_inventory_vector`), drives the particle engine over Oklahoma beside the HRRR 10 m
    /// wind the particles otherwise follow. Prints each field's mean wind over the view and writes
    /// the two particle panels to `target/parity-review/f4-particles/browsed-wind-particles.png`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network and GPU: HRRR 850 hPa and 10 m winds"]
    fn browsed_wind_particles_live() {
        use crate::render::mercator::Camera;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let http = reqwest::Client::new();
        let (run, lead, u, v) = rt
            .block_on(wxdata::hrrr::fetch_inventory_vector(
                &http,
                wxdata::hrrr::Model::Hrrr,
                None,
                1,
                "850 mb",
                wxdata::model_inventory::TimingKind::Instant,
            ))
            .expect("HRRR 850 hPa wind");
        let browsed = particle_field(&(u, v), run, u16::from(lead));
        let (run10, u10, v10) = rt
            .block_on(wxdata::hrrr::fetch_wind(
                &http,
                wxdata::hrrr::WindLevel::Surface,
                1,
            ))
            .expect("HRRR 10 m wind");
        let surface = WindField {
            u: u10,
            v: v10,
            level: wxdata::hrrr::WindLevel::Surface,
            run: run10,
            fcst_hour: 1,
        };
        drop(rt);

        let (vp, cam) = ((480.0f32, 400.0f32), Camera::at_lonlat(-98.0, 35.5, 6.0));
        // Mean wind over a lattice of the view, as east/north m/s.
        let mean = |f: &WindField| {
            let (mut su, mut sv, mut n) = (0.0f64, 0.0f64, 0usize);
            for y in 0..20 {
                for x in 0..24 {
                    let w =
                        cam.screen_to_world((x as f32 * 20.0 + 10.0, y as f32 * 20.0 + 10.0), vp);
                    let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
                    if let Some((u, v)) = f.sample(lon, lat) {
                        (su, sv, n) = (su + f64::from(u), sv + f64::from(v), n + 1);
                    }
                }
            }
            assert!(n > 0, "the view is inside the HRRR domain");
            (su / n as f64, sv / n as f64)
        };
        let mut meshes = Vec::new();
        for (name, f) in [
            ("HRRR 10 m", &surface),
            ("HRRR 850 hPa (browsed)", &browsed),
        ] {
            let (mu, mv) = mean(f);
            eprintln!(
                "{name}, run {} F+{}: mean over the view {:.1} m/s from {:.0}\u{b0}",
                f.run.format("%Y-%m-%d %HZ"),
                f.fcst_hour,
                mu.hypot(mv),
                super::super::model_field::wind_from_deg(mu as f32, mv as f32).unwrap_or(f64::NAN)
            );
            let mut ps = crate::wind_draw::Particles::default();
            for _ in 0..90 {
                ps.update(f, &cam, vp, 1.0 / 30.0);
            }
            let mesh = ps.build_mesh(&cam, vp, egui::Pos2::ZERO, 1.0);
            assert!(!mesh.vertices.is_empty(), "{name}: particles drawn");
            meshes.push((name, mesh));
        }

        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/f4-particles");
        std::fs::create_dir_all(&dir).unwrap();
        gpu.save(&dir.join("browsed-wind-particles.png"), 980, 430, |ui| {
            let painter = ui.painter();
            painter.rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(14, 17, 22));
            for (i, (name, mesh)) in meshes.iter().enumerate() {
                let origin = egui::vec2(10.0 + i as f32 * 490.0, 24.0);
                let mut m = mesh.clone();
                m.translate(origin);
                painter.rect_stroke(
                    egui::Rect::from_min_size(origin.to_pos2(), egui::vec2(vp.0, vp.1)),
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
                    egui::StrokeKind::Inside,
                );
                painter.add(egui::Shape::mesh(m));
                painter.text(
                    egui::pos2(origin.x, 4.0),
                    egui::Align2::LEFT_TOP,
                    format!("{name} \u{b7} Oklahoma, zoom 6 \u{b7} after 90 frames"),
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_gray(225),
                );
            }
        })
        .unwrap();
    }
}
