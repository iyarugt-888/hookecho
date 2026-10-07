//! The on-map ensemble layer: which statistic of which field is shown, how it is colored, and how
//! a hovered value reads (ROADMAP_NEW F7).
//!
//! The 31 member grids are fetched once and kept; picking a different statistic or threshold only
//! recomputes [`wxdata::ensemble::combine`] over them, it never refetches.

use crate::render::{FieldLayer, MrmsUpload};
use crate::settings::TempUnit;
use wxdata::contour::{contour_level, ContourLine};
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
    /// Exceedance threshold in the field's native units (see `EnsembleField::display`); also the
    /// level the spaghetti is drawn at.
    pub threshold: f32,
    /// Every member's contour at `threshold` over whatever statistic is shown.
    pub spaghetti: bool,
}

impl Default for EnsembleView {
    fn default() -> Self {
        let field = EnsembleField::Cape;
        Self {
            field,
            kind: StatKind::Probability,
            threshold: field.default_threshold(),
            spaghetti: false,
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

/// Spaghetti: each member's contour at one level, and the ensemble mean's.
pub struct Spaghetti {
    pub level: f32,
    pub members: Vec<Vec<ContourLine>>,
    pub mean: Vec<ContourLine>,
}

/// What a computed [`Spaghetti`] was made from: field, run, lead, level (by bits) and how many
/// members, so a new run, lead or level is never drawn with the old lines.
pub type SpaghettiKey = (
    EnsembleField,
    chrono::DateTime<chrono::Utc>,
    u16,
    u32,
    usize,
);

pub fn spaghetti_key(view: &EnsembleView, run: &wxdata::ensemble::EnsembleRun) -> SpaghettiKey {
    (
        view.field,
        run.run,
        run.fcst_hour,
        view.threshold.to_bits(),
        run.members.len(),
    )
}

/// Contour every member at `level`, and the members' mean.
pub fn spaghetti(members: &[MrmsField], level: f32) -> Spaghetti {
    let mean = wxdata::ensemble::combine(members, Statistic::Mean)
        .map(|m| contour_level(&m, level))
        .unwrap_or_default();
    Spaghetti {
        level,
        members: members.iter().map(|m| contour_level(m, level)).collect(),
        mean,
    }
}

/// Member `i` of `n`'s line colour: evenly around the colour wheel, so neighbours differ.
pub fn member_color(i: usize, n: usize) -> egui::Color32 {
    let h = i as f32 / n.max(1) as f32;
    let c: egui::Color32 = egui::ecolor::Hsva::new(h, 0.7, 0.95, 1.0).into();
    c.gamma_multiply(0.75)
}

/// The single-model layer whose color scale matches this field's own units.
pub fn source_layer(field: EnsembleField) -> FieldLayer {
    match field {
        EnsembleField::Temp2m => FieldLayer::GlobalTemp2m,
        EnsembleField::Mslp => FieldLayer::GlobalMslp,
        EnsembleField::Height500 => FieldLayer::GlobalHeight500,
        EnsembleField::Cape => FieldLayer::Cape,
        EnsembleField::PrecipitableWater => FieldLayer::GlobalPrecip,
        // The rain layers' own scale, which is millimetres of accumulation too.
        EnsembleField::Precip6h => FieldLayer::Qpe6h,
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
                spaghetti: false,
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
            spaghetti: false,
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

    /// A west-east ramp, value = longitude + `shift`, on 0.5° cells.
    fn ramp(shift: f32) -> MrmsField {
        let (nx, ny) = (40, 10);
        let (west, north) = (-110.0, 45.0);
        let values = (0..ny)
            .flat_map(|_| (0..nx).map(move |c| west as f32 + 0.5 * (c as f32 + 0.5) + shift))
            .collect();
        MrmsField {
            values,
            nx,
            ny,
            lon_west: west,
            lon_east: west + 0.5 * nx as f64,
            lat_north: north,
            lat_south: north - 0.5 * ny as f64,
            time: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    #[test]
    fn each_member_draws_its_own_contour_and_the_mean_sits_between() {
        // Three members whose -100 line falls at -100, -101 and -102 (value = lon + shift).
        let members = [ramp(0.0), ramp(1.0), ramp(2.0)];
        let s = spaghetti(&members, -100.0);
        assert_eq!(s.members.len(), 3);
        let lon_of = |lines: &[ContourLine]| {
            let pts: Vec<f64> = lines
                .iter()
                .flat_map(|l| l.pts.iter().map(|p| p.0))
                .collect();
            assert!(!pts.is_empty());
            pts.iter().sum::<f64>() / pts.len() as f64
        };
        for (m, want) in s.members.iter().zip([-100.0, -101.0, -102.0]) {
            assert!((lon_of(m) - want).abs() < 1e-6, "{} vs {want}", lon_of(m));
        }
        assert!((lon_of(&s.mean) + 101.0).abs() < 1e-6, "the mean's line");
        // Colours differ from member to member.
        assert_ne!(member_color(0, 31), member_color(1, 31));
    }

    #[test]
    fn spaghetti_is_redrawn_for_a_new_level_but_not_for_a_new_statistic() {
        let run = wxdata::ensemble::EnsembleRun {
            members: vec![ramp(0.0), ramp(1.0)],
            run: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            fcst_hour: 24,
        };
        let mut v = EnsembleView {
            spaghetti: true,
            ..EnsembleView::default()
        };
        let k = spaghetti_key(&v, &run);
        v.kind = StatKind::Mean;
        assert_eq!(spaghetti_key(&v, &run), k);
        v.threshold += 1.0;
        assert_ne!(spaghetti_key(&v, &run), k);
    }

    /// The GEFS 500 hPa height spaghetti at 5700 m, live: all 31 members have lines.
    /// `cargo test -p hookecho --lib gefs_spaghetti_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn gefs_spaghetti_live() {
        let http = reqwest::Client::new();
        let run = wxdata::ensemble::fetch_gefs(&http, EnsembleField::Height500, 48)
            .await
            .unwrap();
        let s = spaghetti(&run.members, 5_700.0);
        let points: Vec<usize> = s
            .members
            .iter()
            .map(|m| m.iter().map(|l| l.pts.len()).sum())
            .collect();
        println!(
            "GEFS {} F+48: {} members, contour points per member {:?}; mean {} lines",
            run.run,
            run.members.len(),
            points,
            s.mean.len()
        );
        assert!(points.iter().all(|&p| p > 100));
        assert!(!s.mean.is_empty());
    }
}
