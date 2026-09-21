//! The on-map ensemble layer: which statistic of which field is shown, how it is colored, and how
//! a hovered value reads (ROADMAP_NEW F7).
//!
//! The 31 member grids are fetched once and kept; picking a different statistic or threshold only
//! recomputes [`wxdata::ensemble::combine`] over them, it never refetches.

use crate::render::{FieldLayer, MrmsUpload};
use crate::settings::TempUnit;
use wxdata::ensemble::{EnsembleField, Statistic};
use wxdata::mrms::MrmsField;

/// Which statistic the layer shows. [`Statistic`] carries its parameters; this is the choice a
/// person makes, with the threshold held beside it in [`EnsembleView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatKind {
    Mean,
    Spread,
    Min,
    Max,
    P10,
    P90,
    Probability,
}

impl StatKind {
    pub const ALL: [StatKind; 7] = [
        StatKind::Mean,
        StatKind::Spread,
        StatKind::Min,
        StatKind::Max,
        StatKind::P10,
        StatKind::P90,
        StatKind::Probability,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatKind::Mean => "Mean",
            StatKind::Spread => "Spread",
            StatKind::Min => "Min",
            StatKind::Max => "Max",
            StatKind::P10 => "10th pct",
            StatKind::P90 => "90th pct",
            StatKind::Probability => "Probability",
        }
    }
}

/// What the layer shows right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnsembleView {
    pub field: EnsembleField,
    pub kind: StatKind,
    /// Exceedance threshold in the field's native units (see `EnsembleField::display`).
    pub threshold: f32,
}

impl Default for EnsembleView {
    fn default() -> Self {
        let field = EnsembleField::Cape;
        Self {
            field,
            kind: StatKind::Probability,
            threshold: field.default_threshold(),
        }
    }
}

impl EnsembleView {
    /// Change the field, resetting the threshold: a CAPE threshold means nothing for MSLP.
    pub fn set_field(&mut self, field: EnsembleField) {
        if self.field != field {
            self.field = field;
            self.threshold = field.default_threshold();
        }
    }

    pub fn statistic(&self) -> Statistic {
        match self.kind {
            StatKind::Mean => Statistic::Mean,
            StatKind::Spread => Statistic::Spread,
            StatKind::Min => Statistic::Min,
            StatKind::Max => Statistic::Max,
            StatKind::P10 => Statistic::Percentile(10),
            StatKind::P90 => Statistic::Percentile(90),
            StatKind::Probability => Statistic::ProbabilityAbove(self.threshold),
        }
    }

    /// Everything that changes the displayed grid without changing the fetched members. The
    /// threshold only matters to the probability statistic, and is compared by bits so this stays
    /// `Eq`.
    pub fn display_key(&self) -> (EnsembleField, StatKind, u32) {
        let threshold = if self.kind == StatKind::Probability {
            self.threshold.to_bits()
        } else {
            0
        };
        (self.field, self.kind, threshold)
    }

    /// A short name for the legend and the stamp.
    pub fn title(&self, temp_unit: TempUnit) -> String {
        match self.kind {
            StatKind::Probability => {
                let (value, unit) = self.threshold_display(temp_unit);
                format!(
                    "GEFS {} — chance above {value:.0} {unit}",
                    self.field.label()
                )
            }
            _ => format!("GEFS {} — {}", self.field.label(), self.kind.label()),
        }
    }

    /// The threshold as a person reads it, in their temperature unit where that applies.
    pub fn threshold_display(&self, temp_unit: TempUnit) -> (f32, &'static str) {
        if self.field == EnsembleField::Temp2m {
            let c = self.field.to_display(self.threshold);
            (temp_unit.from_c(c), temp_unit.label())
        } else {
            (
                self.field.to_display(self.threshold),
                self.field.display().0,
            )
        }
    }

    /// Inverse of [`Self::threshold_display`], for an edited threshold.
    pub fn set_threshold_display(&mut self, shown: f32, temp_unit: TempUnit) {
        let display = if self.field == EnsembleField::Temp2m {
            match temp_unit {
                TempUnit::Fahrenheit => (shown - 32.0) * 5.0 / 9.0,
                TempUnit::Celsius => shown,
            }
        } else {
            shown
        };
        self.threshold = self.field.from_display(display);
    }
}

/// The single-model layer whose color scale matches this field's own units.
pub fn source_layer(field: EnsembleField) -> FieldLayer {
    match field {
        EnsembleField::Temp2m => FieldLayer::GlobalTemp2m,
        EnsembleField::Mslp => FieldLayer::GlobalMslp,
        EnsembleField::Height500 => FieldLayer::GlobalHeight500,
        EnsembleField::Cape => FieldLayer::Cape,
        EnsembleField::PrecipitableWater => FieldLayer::GlobalPrecip,
    }
}

/// Whether the statistic is in the field's own units (and so wears the field's own ramp), as
/// opposed to a spread or a percentage, which have scales of their own.
pub fn uses_field_ramp(kind: StatKind) -> bool {
    !matches!(kind, StatKind::Spread | StatKind::Probability)
}

/// Full scale of the sequential ramp: the field's spread scale, or 100 for a probability.
pub fn sequential_full_scale(view: &EnsembleView) -> f32 {
    match view.kind {
        StatKind::Spread => view.field.spread_full_scale(),
        _ => 100.0,
    }
}

/// The sequential ramp's stops, shared by the texture and the legend so they cannot drift apart.
pub const SEQUENTIAL_STOPS: [(f32, [u8; 3]); 4] = [
    (0.0, [255, 255, 200]),
    (0.35, [255, 200, 60]),
    (0.7, [230, 90, 40]),
    (1.0, [150, 20, 170]),
];
/// Translucent, so coastlines and borders stay readable underneath.
pub const SEQUENTIAL_ALPHA: u8 = 190;

/// Index 0 is the clear slot; below 2% of full scale is noise rather than signal.
pub fn sequential_index(value: f32, full_scale: f32) -> u8 {
    let t = (value / full_scale).clamp(0.0, 1.0);
    if t < 0.02 {
        0
    } else {
        (1.0 + t * 254.0) as u8
    }
}

/// The GPU upload for `grid`, the statistic `view` describes.
pub fn upload(grid: &MrmsField, view: &EnsembleView) -> MrmsUpload {
    if uses_field_ramp(view.kind) {
        return crate::app::field_upload_indexed(source_layer(view.field), grid);
    }
    let full = sequential_full_scale(view);
    crate::app::field_index_upload(
        grid,
        |v| sequential_index(v, full),
        crate::app::ramp_lut_a(&SEQUENTIAL_STOPS, SEQUENTIAL_ALPHA),
    )
}

/// A hovered value, worded for a person: a percentage, a spread in the reader's units, or the
/// field's own reading.
pub fn format_value(view: &EnsembleView, raw: f32, temp_unit: TempUnit) -> Option<String> {
    if !raw.is_finite() {
        return None;
    }
    let field = view.field;
    Some(match view.kind {
        StatKind::Probability => format!("{raw:.0}%"),
        StatKind::Spread => {
            let native = field.spread_to_display(raw);
            if field == EnsembleField::Temp2m {
                // A spread is a difference: convert the size of the degree, not the zero point.
                let scaled = temp_unit.from_c(native) - temp_unit.from_c(0.0);
                format!("±{scaled:.1} {}", temp_unit.label())
            } else {
                format!("±{native:.1} {}", field.display().0)
            }
        }
        _ if field == EnsembleField::Temp2m => {
            format!(
                "{:.1} {}",
                temp_unit.from_c(field.to_display(raw)),
                temp_unit.label()
            )
        }
        _ => format!("{:.1} {}", field.to_display(raw), field.display().0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_the_field_resets_the_threshold_but_repeating_it_does_not() {
        let mut v = EnsembleView {
            threshold: 2500.0,
            ..EnsembleView::default()
        };
        v.set_field(EnsembleField::Cape);
        assert_eq!(v.threshold, 2500.0, "same field keeps an edited threshold");
        v.set_field(EnsembleField::Mslp);
        assert_eq!(v.threshold, EnsembleField::Mslp.default_threshold());
    }

    #[test]
    fn the_threshold_only_matters_to_the_probability_statistic() {
        let mut a = EnsembleView {
            kind: StatKind::Mean,
            ..EnsembleView::default()
        };
        let before = a.display_key();
        a.threshold += 500.0;
        assert_eq!(a.display_key(), before);
        a.kind = StatKind::Probability;
        let p = a.display_key();
        a.threshold += 500.0;
        assert_ne!(a.display_key(), p);
    }

    #[test]
    fn a_typed_threshold_round_trips_in_either_temperature_unit() {
        for unit in [TempUnit::Fahrenheit, TempUnit::Celsius] {
            let mut v = EnsembleView {
                field: EnsembleField::Temp2m,
                kind: StatKind::Probability,
                threshold: 0.0,
            };
            let freezing = if unit == TempUnit::Fahrenheit {
                32.0
            } else {
                0.0
            };
            v.set_threshold_display(freezing, unit);
            assert!(
                (v.threshold - 273.15).abs() < 1e-3,
                "{unit:?}: {}",
                v.threshold
            );
            let (shown, _) = v.threshold_display(unit);
            assert!((shown - freezing).abs() < 1e-3);
        }
    }

    #[test]
    fn readouts_use_the_readers_units() {
        let v = |field, kind| EnsembleView {
            field,
            kind,
            threshold: 0.0,
        };
        let f = TempUnit::Fahrenheit;
        assert_eq!(
            format_value(&v(EnsembleField::Temp2m, StatKind::Mean), 273.15, f).as_deref(),
            Some("32.0 °F")
        );
        // A 5 °C spread is 9 °F wide, not 41 °F.
        assert_eq!(
            format_value(&v(EnsembleField::Temp2m, StatKind::Spread), 5.0, f).as_deref(),
            Some("±9.0 °F")
        );
        assert_eq!(
            format_value(&v(EnsembleField::Cape, StatKind::Probability), 62.4, f).as_deref(),
            Some("62%")
        );
        assert_eq!(
            format_value(&v(EnsembleField::Mslp, StatKind::Mean), 101_325.0, f).as_deref(),
            Some("1013.2 hPa")
        );
        assert_eq!(
            format_value(&v(EnsembleField::Mslp, StatKind::Mean), f32::NAN, f),
            None
        );
    }

    #[test]
    fn the_sequential_ramp_is_clear_at_noise_and_full_at_scale() {
        assert_eq!(sequential_index(0.0, 100.0), 0);
        assert_eq!(sequential_index(1.9, 100.0), 0);
        assert!(sequential_index(2.1, 100.0) >= 1);
        assert_eq!(sequential_index(100.0, 100.0), 255);
        assert_eq!(sequential_index(1e9, 100.0), 255);
        assert_eq!(sequential_index(-5.0, 100.0), 0);
    }
}
