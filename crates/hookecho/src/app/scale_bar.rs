//! The map scale bar: a ruler in the corner of each flat pane, a round distance long, in the
//! pane's units (miles over a NEXRAD or TDWR, kilometres elsewhere, as the readouts use).
//! Only where one length holds across the screen: a tilted or globe camera has none.

use super::HookEchoApp;
use crate::render::mercator;

/// The ruler never runs longer than this.
const MAX_W: f32 = 120.0;
const METRES_PER_MILE: f64 = 1609.344;
const METRES_PER_FOOT: f64 = 0.3048;

/// The longest round length (1, 2 or 5 × a power of ten, in the units) that fits in `max_pt`
/// at `metres_per_pt`: its width in points and its label. Below one mile it counts in feet,
/// below one kilometre in metres.
pub(super) fn ruler(metres_per_pt: f64, max_pt: f32, metric: bool) -> Option<(f32, String)> {
    if !(metres_per_pt.is_finite() && metres_per_pt > 0.0) {
        return None;
    }
    let max_m = metres_per_pt * f64::from(max_pt);
    let (big, big_unit, small, small_unit) = if metric {
        (1000.0, "km", 1.0, "m")
    } else {
        (METRES_PER_MILE, "mi", METRES_PER_FOOT, "ft")
    };
    let (unit_m, unit) = if max_m >= big {
        (big, big_unit)
    } else {
        (small, small_unit)
    };
    let max_units = max_m / unit_m;
    let pow = 10f64.powf(max_units.log10().floor());
    let len = [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * pow)
        .find(|l| *l <= max_units)?;
    let label = if len >= 1.0 {
        format!("{len:.0} {unit}")
    } else {
        format!("{len} {unit}")
    };
    Some(((len * unit_m / metres_per_pt) as f32, label))
}

impl HookEchoApp {
    /// A scale bar in the lower left of each flat pane, while the setting is on.
    pub(crate) fn paint_scale_bar(&self, ui: &egui::Ui, rects: &[egui::Rect]) {
        if !self.settings.scale_bar || !self.workstation_chrome() || self.phone_station() {
            return;
        }
        for (idx, rect) in rects.iter().enumerate() {
            let cam = &self.views[idx].camera;
            if cam.is_3d() || cam.globe_blend() > 0.0 {
                continue;
            }
            let (_, lat) = mercator::world_to_lonlat(cam.center.0, cam.center.1);
            let metres_per_pt =
                cam.world_per_pixel() / mercator::Camera::world_units_per_metre(lat);
            let Some((w, label)) = ruler(metres_per_pt, MAX_W, self.metric_in(idx)) else {
                continue;
            };
            let painter = ui.painter_at(*rect);
            // Above the basemap attribution along the bottom edge.
            let y = rect.bottom() - 34.0;
            let x0 = rect.left() + 14.0;
            let x1 = x0 + w;
            let path = [
                egui::pos2(x0, y - 5.0),
                egui::pos2(x0, y),
                egui::pos2(x1, y),
                egui::pos2(x1, y - 5.0),
            ];
            // A dark halo under a light line reads over any basemap and any radar colour.
            let halo = egui::Color32::from_black_alpha(200);
            painter.line(path.to_vec(), egui::Stroke::new(3.5, halo));
            painter.line(path.to_vec(), egui::Stroke::new(1.5, egui::Color32::WHITE));
            let font = egui::FontId::monospace(11.0);
            let at = egui::pos2((x0 + x1) / 2.0, y - 4.0);
            for d in [
                egui::vec2(-1.0, 0.0),
                egui::vec2(1.0, 0.0),
                egui::vec2(0.0, -1.0),
                egui::vec2(0.0, 1.0),
            ] {
                painter.text(
                    at + d,
                    egui::Align2::CENTER_BOTTOM,
                    &label,
                    font.clone(),
                    halo,
                );
            }
            painter.text(
                at,
                egui::Align2::CENTER_BOTTOM,
                &label,
                font,
                egui::Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ruler;

    #[test]
    fn the_ruler_is_the_longest_round_length_that_fits() {
        // 100 m a point, 120 points: 12 km fits, so 10 km.
        let (w, label) = ruler(100.0, 120.0, true).unwrap();
        assert_eq!(label, "10 km");
        assert!((w - 100.0).abs() < 1e-3);
        // 7.46 mi fits: 5 mi.
        assert_eq!(ruler(100.0, 120.0, false).unwrap().1, "5 mi");
        // 2 m a point: 240 m fits, so 200 m; in feet, 787 ft fits, so 500 ft.
        assert_eq!(ruler(2.0, 120.0, true).unwrap().1, "200 m");
        assert_eq!(ruler(2.0, 120.0, false).unwrap().1, "500 ft");
        // Zoomed out: 1200 km fits, so 1000 km.
        assert_eq!(ruler(10_000.0, 120.0, true).unwrap().1, "1000 km");
    }

    #[test]
    fn the_ruler_is_never_longer_than_its_room() {
        for mpp in [0.3, 1.7, 13.0, 99.0, 777.0, 4321.0] {
            for metric in [true, false] {
                let (w, _) = ruler(mpp, 120.0, metric).unwrap();
                assert!(w <= 120.0 && w > 120.0 / 5.0 - 1.0, "{mpp} {metric}: {w}");
            }
        }
        assert!(ruler(0.0, 120.0, true).is_none());
        assert!(ruler(f64::NAN, 120.0, true).is_none());
    }
}
