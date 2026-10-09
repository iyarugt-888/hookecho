//! The quality profile row (ROADMAP_PARITY M5.4, 1008.md E4): Low, Balanced, High and Analysis set
//! every pane's 3D quality, the 3D window's, display blending and the wind particle share in one
//! choice (`crate::quality`). The row says "Custom" when the knobs were set one by one.

use super::*;
use crate::quality::{QualityProfile, RenderPolicy};

/// Put profile `q`'s knobs on the panes, the 3D window and the settings. Only those knobs: the
/// products, thresholds, colour tables, time and everything a probe or an export reads stay as
/// they are.
pub(crate) fn apply_profile(
    q: QualityProfile,
    views: &mut [MapView],
    window3d_steps: &mut u32,
    settings: &mut crate::settings::Settings,
) {
    let p = q.policy();
    for v in views {
        v.map_3d.quality_steps = p.map3d_steps;
        v.smooth = p.smooth;
    }
    *window3d_steps = p.window3d_steps;
    settings.smooth_radar = p.smooth;
    settings.wind_particle_pct = p.particle_pct;
}

/// The knobs as they stand, read from pane `active` (panes set one by one may differ; the row
/// describes the one being looked at).
pub(crate) fn policy_now(
    view: &MapView,
    window3d_steps: u32,
    settings: &crate::settings::Settings,
) -> RenderPolicy {
    RenderPolicy {
        map3d_steps: view.map_3d.quality_steps,
        window3d_steps,
        smooth: settings.smooth_radar,
        particle_pct: settings.wind_particle_pct,
    }
}

impl HookEchoApp {
    /// The profile row: four choices, the one in effect selected, or "Custom".
    pub(crate) fn quality_row(&mut self, ui: &mut egui::Ui) {
        let now = policy_now(&self.views[self.active], self.vol3d.steps, &self.settings);
        if let Some(q) = quality_row_body(ui, QualityProfile::matching(&now)) {
            apply_profile(
                q,
                &mut self.views,
                &mut self.vol3d.steps,
                &mut self.settings,
            );
            self.settings.save();
        }
    }
}

/// The row itself: the four profiles with `current` selected, or "Custom" when none is. Returns
/// the one clicked.
fn quality_row_body(ui: &mut egui::Ui, current: Option<QualityProfile>) -> Option<QualityProfile> {
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("Quality");
        for q in QualityProfile::ALL {
            if ui
                .selectable_label(current == Some(q), q.label())
                .on_hover_text(q.hint())
                .clicked()
            {
                pick = Some(q);
            }
        }
        if current.is_none() {
            ui.weak("Custom").on_hover_text(
                "The 3D quality, smoothing or wind particles were set one by one; pick a \
                 profile to set them together",
            );
        }
    });
    ui.weak("Changes how things are drawn, never a value a probe or an export reads.");
    pick
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::scenes::{scene_product, scene_time};

    fn pane() -> MapView {
        let mut v = MapView::new(
            Some("KTLX".into()),
            crate::render::mercator::Camera::at_lonlat(-97.3, 35.3, 8.0),
        );
        v.moment = Moment::Velocity;
        v.tilt = 2;
        v.thresholds[Moment::Velocity.index()] = Some(20.0);
        v.threshold_enabled[Moment::Velocity.index()] = true;
        v
    }

    /// The contract: a profile sets its knobs and nothing that decides a value.
    #[test]
    fn a_profile_changes_how_things_draw_and_never_what_they_are() {
        for q in QualityProfile::ALL {
            let mut views = vec![pane(), pane()];
            let mut settings = crate::settings::Settings::default();
            settings
                .palettes
                .insert("REF".into(), "builtin:High contrast (reflectivity)".into());
            let science: Vec<_> = views
                .iter()
                .map(|v| (scene_product(v), scene_time(v)))
                .collect();
            let before = settings.clone();
            let mut window = 1u32;
            apply_profile(q, &mut views, &mut window, &mut settings);
            let after: Vec<_> = views
                .iter()
                .map(|v| (scene_product(v), scene_time(v)))
                .collect();
            assert_eq!(after, science, "{q:?} changed a product, threshold or time");
            assert_eq!(
                crate::settings::Settings {
                    smooth_radar: before.smooth_radar,
                    wind_particle_pct: before.wind_particle_pct,
                    ..settings.clone()
                },
                before,
                "{q:?} changed a setting beyond its knobs"
            );
            assert_eq!(
                QualityProfile::matching(&policy_now(&views[1], window, &settings)),
                Some(q)
            );
            assert!(views.iter().all(|v| v.smooth == q.policy().smooth));
        }
    }

    /// The row with a profile in effect and with knobs set by hand, for review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the quality row"]
    fn gpu_quality_row_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the row");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m5.4");
        std::fs::create_dir_all(&destination).unwrap();
        for (file, current) in [
            ("quality-analysis.png", Some(QualityProfile::Analysis)),
            ("quality-custom.png", None),
        ] {
            gpu.save(&destination.join(file), 420, 80, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(400.0);
                    quality_row_body(ui, current);
                });
            })
            .unwrap();
        }
    }

    #[test]
    fn a_knob_set_by_hand_reads_as_custom() {
        let mut views = vec![pane()];
        let mut settings = crate::settings::Settings::default();
        let mut window = 0;
        apply_profile(
            QualityProfile::Balanced,
            &mut views,
            &mut window,
            &mut settings,
        );
        views[0].map_3d.quality_steps = 128;
        assert_eq!(
            QualityProfile::matching(&policy_now(&views[0], window, &settings)),
            None
        );
    }
}
