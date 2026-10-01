//! Shared physical processing and display units for radar pointer readouts.
use crate::settings::VelocityUnit;
use wxdata::level2::Moment;

impl super::HookEchoApp {
    /// Top visible field, otherwise the radar gate with the selected display processing.
    pub(super) fn probe_row(
        &mut self,
        ctx: &egui::Context,
        idx: usize,
        lon: f64,
        lat: f64,
        vp: (f32, f32),
    ) -> crate::ui::cursor_probe::ProbeRow {
        if let Some(layer) = self.probe_field(idx, lon, lat, vp) {
            return self.grid_probe_row(idx, layer, lon, lat);
        }
        let moment = self.views[idx].moment;
        let storm_uv = self.views[idx].storm_motion_uv();
        let product = if storm_uv.is_some() {
            "SRV"
        } else {
            moment.short_name()
        };
        let dealias = self.settings.dealias_velocity
            && moment == Moment::Velocity
            && !self.views[idx]
                .site
                .as_deref()
                .is_some_and(wxdata::tdwr::is_tdwr);
        match self.inspect_gate(ctx, idx, lon, lat, None) {
            Some(popup) => crate::ui::cursor_probe::ProbeRow {
                pane: idx,
                source: popup.site.unwrap_or_else(|| "—".into()),
                product: product.into(),
                time: popup
                    .inspection
                    .sample
                    .collected_ms
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .or_else(|| popup.time_range.map(|(_, end)| end)),
                value: format_value(
                    moment,
                    relative_value(
                        moment,
                        if dealias {
                            popup.inspection.dealiased_value
                        } else {
                            popup.inspection.sample.value
                        },
                        popup.inspection.sample.azimuth_deg,
                        storm_uv,
                    ),
                    self.settings.velocity_unit,
                ),
                folded: popup.inspection.sample.folded
                    && !(dealias && popup.inspection.dealiased_value.is_some()),
            },
            None => crate::ui::cursor_probe::ProbeRow {
                pane: idx,
                source: self.views[idx].site.clone().unwrap_or_else(|| "—".into()),
                product: product.into(),
                time: None,
                value: None,
                folded: false,
            },
        }
    }
}

/// The same radial projection subtracted by the radar shaders. Missing and nonfinite input
/// stays missing; a remembered SRV setting never transforms a non-velocity moment.
pub(super) fn relative_value(
    moment: Moment,
    value: Option<f32>,
    azimuth_deg: f32,
    storm_uv: Option<(f32, f32)>,
) -> Option<f32> {
    let mut value = value.filter(|v| v.is_finite())?;
    if moment == Moment::Velocity {
        if let Some((east, north)) = storm_uv {
            let azimuth = azimuth_deg.to_radians();
            value -= east * azimuth.sin() + north * azimuth.cos();
        }
    }
    value.is_finite().then_some(value)
}

pub(super) fn display_units(moment: Moment, unit: VelocityUnit) -> (f32, &'static str) {
    if matches!(moment, Moment::Velocity | Moment::SpectrumWidth) {
        (unit.factor_from_ms(), unit.label())
    } else {
        (1.0, moment.units())
    }
}

pub(super) fn format_value(
    moment: Moment,
    value: Option<f32>,
    unit: VelocityUnit,
) -> Option<String> {
    let (factor, units) = display_units(moment, unit);
    let value = value.filter(|v| v.is_finite())? * factor;
    let number = if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    Some(format!("{number} {units}").trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_velocity_subtracts_the_radial_motion_component() {
        for (azimuth, expected) in [(0.0, 16.0), (90.0, 17.0), (180.0, 24.0), (270.0, 23.0)] {
            let got =
                relative_value(Moment::Velocity, Some(20.0), azimuth, Some((3.0, 4.0))).unwrap();
            assert!((got - expected).abs() < 1e-5, "azimuth {azimuth}: {got}");
        }
        // Northeast beam, with a purely eastward storm: subtract only its radial component.
        let got = relative_value(Moment::Velocity, Some(20.0), 45.0, Some((10.0, 0.0))).unwrap();
        assert!((got - (20.0 - 10.0 / 2.0_f32.sqrt())).abs() < 1e-5);
    }

    #[test]
    fn non_velocity_and_missing_samples_do_not_acquire_relative_values() {
        for moment in Moment::ALL {
            assert_eq!(relative_value(moment, None, 0.0, Some((3.0, 4.0))), None);
            assert_eq!(relative_value(moment, Some(f32::NAN), 0.0, None), None);
            if moment != Moment::Velocity {
                assert_eq!(
                    relative_value(moment, Some(0.95), 0.0, Some((3.0, 4.0))),
                    Some(0.95)
                );
            }
        }
        assert_eq!(
            relative_value(Moment::Velocity, Some(20.0), 0.0, Some((0.0, f32::NAN))),
            None
        );
    }

    #[test]
    fn displayed_units_preserve_small_values_and_unitless_precision() {
        for moment in [Moment::Velocity, Moment::SpectrumWidth] {
            assert_eq!(
                format_value(moment, Some(10.0), VelocityUnit::Knots).unwrap(),
                "19.4 kt"
            );
            assert_eq!(
                format_value(moment, Some(10.0), VelocityUnit::Mph).unwrap(),
                "22.4 mph"
            );
            assert_eq!(
                format_value(moment, Some(-0.25), VelocityUnit::MetersPerSecond).unwrap(),
                "-0.25 m/s"
            );
        }
        assert_eq!(
            format_value(
                Moment::CorrelationCoefficient,
                Some(0.95),
                VelocityUnit::Knots
            )
            .unwrap(),
            "0.95"
        );
        assert_eq!(
            format_value(
                Moment::SpecificDifferentialPhase,
                Some(1.4),
                VelocityUnit::Mph
            )
            .unwrap(),
            "1.40 deg/km"
        );
    }
}
