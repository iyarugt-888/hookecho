//! Ensemble statistics over member grids (ROADMAP_NEW F7).
//!
//! An ensemble is many equally likely runs of one model from slightly different starting states.
//! The members themselves are rarely the point; what forecasters read is what the *set* says: the
//! mean, how far the members disagree (spread), the range they cover, and — most useful of all —
//! what fraction of them cross a threshold that matters.
//!
//! [`combine`] is pure math over [`MrmsField`] grids that share one lattice, so it is exercised
//! offline. [`fetch_gefs`] gathers the GEFS members for it.
//!
//! Missing data is handled per cell: a member with no value there is left out, and a cell where
//! fewer than half the members have one is reported missing rather than as a statistic of a
//! handful of survivors.

use crate::mrms::MrmsField;
use chrono::{DateTime, Utc};

/// Control plus thirty perturbed members.
pub const GEFS_MEMBERS: u8 = 31;

/// What to compute across the members at every grid cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Statistic {
    Mean,
    /// Sample standard deviation across members — the usual "spread" shown beside the mean.
    Spread,
    Min,
    Max,
    /// Percentile 0..=100, linearly interpolated between the two nearest ranked members.
    Percentile(u8),
    /// Percent (0..=100) of members strictly above the threshold, in the field's native units.
    ProbabilityAbove(f32),
}

impl Statistic {
    /// Parse a short spec: `mean`, `spread`, `min`, `max`, `p90` (percentile), `prob` (above the
    /// field's default threshold) or `prob:1000` (above 1000, in the field's native units).
    pub fn parse(spec: &str, field: EnsembleField) -> Option<Statistic> {
        let s = spec.trim().to_ascii_lowercase();
        match s.as_str() {
            "mean" => Some(Statistic::Mean),
            "spread" => Some(Statistic::Spread),
            "min" => Some(Statistic::Min),
            "max" => Some(Statistic::Max),
            "prob" => Some(Statistic::ProbabilityAbove(field.default_threshold())),
            _ => {
                if let Some(t) = s.strip_prefix("prob:") {
                    t.parse().ok().map(Statistic::ProbabilityAbove)
                } else {
                    let p: u8 = s.strip_prefix('p')?.parse().ok()?;
                    (p <= 100).then_some(Statistic::Percentile(p))
                }
            }
        }
    }

    pub fn label(self) -> String {
        match self {
            Statistic::Mean => "Ensemble mean".into(),
            Statistic::Spread => "Ensemble spread (std dev)".into(),
            Statistic::Min => "Ensemble minimum".into(),
            Statistic::Max => "Ensemble maximum".into(),
            Statistic::Percentile(p) => format!("{p}th percentile"),
            Statistic::ProbabilityAbove(t) => format!("Probability above {t}"),
        }
    }
}

/// A field the ensemble path can fetch, by meaning. GEFS names go here and nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnsembleField {
    Temp2m,
    Mslp,
    Height500,
    /// 180–0 mb mixed-layer CAPE.
    Cape,
    PrecipitableWater,
    /// Precipitation accumulated over the six hours ending at the lead (QPF).
    Precip6h,
}

impl EnsembleField {
    pub const ALL: [EnsembleField; 6] = [
        EnsembleField::Temp2m,
        EnsembleField::Mslp,
        EnsembleField::Height500,
        EnsembleField::Cape,
        EnsembleField::PrecipitableWater,
        EnsembleField::Precip6h,
    ];

    /// Whether the members publish this field at forecast hour `fh`. The 6-hour accumulation exists
    /// only at multiples of six and not at hour zero (nothing has accumulated yet); everything else
    /// is a snapshot available at every published lead.
    pub fn valid_lead(self, fh: u16) -> bool {
        match self {
            EnsembleField::Precip6h => fh >= 6 && fh.is_multiple_of(6),
            _ => true,
        }
    }

    /// The nearest published lead at or after `fh`, for a lead chosen on a finer or different grid
    /// than this field allows.
    pub fn snap_lead(self, fh: u16) -> u16 {
        match self {
            EnsembleField::Precip6h => fh.div_ceil(6).max(1) * 6,
            _ => fh,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EnsembleField::Temp2m => "2 m temperature",
            EnsembleField::Mslp => "MSLP",
            EnsembleField::Height500 => "500 hPa height",
            EnsembleField::Cape => "Mixed-layer CAPE",
            EnsembleField::PrecipitableWater => "Precipitable water",
            EnsembleField::Precip6h => "6-hour rain (QPF)",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            EnsembleField::Temp2m => "t2m",
            EnsembleField::Mslp => "mslp",
            EnsembleField::Height500 => "gh500",
            EnsembleField::Cape => "cape",
            EnsembleField::PrecipitableWater => "pwat",
            EnsembleField::Precip6h => "qpf6",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.slug() == s)
    }

    /// GEFS `.idx` `(var, level)`.
    fn gefs_key(self) -> (&'static str, &'static str) {
        match self {
            EnsembleField::Temp2m => ("TMP", "2 m above ground"),
            EnsembleField::Mslp => ("PRMSL", "mean sea level"),
            EnsembleField::Height500 => ("HGT", "500 mb"),
            EnsembleField::Cape => ("CAPE", "180-0 mb above ground"),
            EnsembleField::PrecipitableWater => {
                ("PWAT", "entire atmosphere (considered as a single layer)")
            }
            // One message per file: the accumulation over the six hours ending at that lead.
            EnsembleField::Precip6h => ("APCP", "surface"),
        }
    }

    /// Native units, in the same terms the grids arrive in (Kelvin, Pa, gpm, J/kg, kg/m²).
    pub fn native_units(self) -> &'static str {
        match self {
            EnsembleField::Temp2m => "K",
            EnsembleField::Mslp => "Pa",
            EnsembleField::Height500 => "gpm",
            EnsembleField::Cape => "J/kg",
            EnsembleField::PrecipitableWater => "kg/m²",
            EnsembleField::Precip6h => "kg/m²",
        }
    }

    /// How to show a native value to a person: `(unit label, scale, offset)` with
    /// `shown = native * scale + offset`. Kelvin and pascals are not what anyone types.
    pub fn display(self) -> (&'static str, f32, f32) {
        match self {
            EnsembleField::Temp2m => ("°C", 1.0, -273.15),
            EnsembleField::Mslp => ("hPa", 0.01, 0.0),
            EnsembleField::Height500 => ("gpm", 1.0, 0.0),
            EnsembleField::Cape => ("J/kg", 1.0, 0.0),
            // A kilogram of water per square metre is a millimetre of depth.
            EnsembleField::PrecipitableWater | EnsembleField::Precip6h => ("mm", 1.0, 0.0),
        }
    }

    /// A native value in display units.
    pub fn to_display(self, native: f32) -> f32 {
        let (_, scale, offset) = self.display();
        native * scale + offset
    }

    /// Display units back to native, for a threshold the user typed.
    pub fn from_display(self, shown: f32) -> f32 {
        let (_, scale, offset) = self.display();
        (shown - offset) / scale
    }

    /// A *difference* (spread) in display units: the offset does not apply to a range.
    pub fn spread_to_display(self, native: f32) -> f32 {
        native * self.display().1
    }

    /// Member spread (std dev, native units) at which a spread map reaches full color. Eyeballed
    /// from what a genuinely uncertain day-3 forecast looks like for each field.
    pub fn spread_full_scale(self) -> f32 {
        match self {
            EnsembleField::Temp2m => 8.0,
            EnsembleField::Mslp => 800.0,
            EnsembleField::Height500 => 60.0,
            EnsembleField::Cape => 1_000.0,
            EnsembleField::PrecipitableWater => 10.0,
            EnsembleField::Precip6h => 10.0,
        }
    }

    /// A threshold worth asking "what fraction of members exceed this?" about, in native units.
    pub fn default_threshold(self) -> f32 {
        match self {
            EnsembleField::Temp2m => 273.15, // freezing
            EnsembleField::Mslp => 100_000.0,
            EnsembleField::Height500 => 5_700.0,
            EnsembleField::Cape => 1_000.0,
            EnsembleField::PrecipitableWater => 40.0,
            // Half an inch in six hours: enough to matter for ponding and rises on small streams.
            EnsembleField::Precip6h => 12.7,
        }
    }
}

/// The member grids of one ensemble run, all on one lattice and one valid time.
pub struct EnsembleRun {
    pub members: Vec<MrmsField>,
    pub run: DateTime<Utc>,
    pub fcst_hour: u16,
}

impl EnsembleRun {
    pub fn valid(&self) -> DateTime<Utc> {
        self.run + chrono::Duration::hours(self.fcst_hour as i64)
    }
}

/// Fetch every GEFS member of `field` at `fh` from one cycle.
///
/// The control member finds the cycle; the rest are pinned to it, so members can never come from
/// different runs. A member that fails to arrive is left out (the statistic then rests on fewer
/// members), but fewer than [`MIN_MEMBERS`] is an error rather than a thin ensemble presented as
/// a full one.
pub async fn fetch_gefs(
    http: &reqwest::Client,
    field: EnsembleField,
    fh: u16,
) -> anyhow::Result<EnsembleRun> {
    anyhow::ensure!(
        field.valid_lead(fh),
        "{} is only published at multiples of six hours after the start, not F+{fh}h",
        field.label()
    );
    let key = field.gefs_key();
    let (run, control) = crate::global::find_gefs_cycle(http, key, fh).await?;
    let mut members = vec![control];
    let numbers: Vec<u8> = (1..GEFS_MEMBERS).collect();
    // Small batches: thirty simultaneous requests to one bucket is rude and gains nothing.
    for batch in numbers.chunks(10) {
        let got = futures_util::future::join_all(
            batch
                .iter()
                .map(|&m| crate::global::fetch_gefs_member(http, key, run, fh, m)),
        )
        .await;
        members.extend(got.into_iter().filter_map(Result::ok));
    }
    anyhow::ensure!(
        members.len() >= MIN_MEMBERS,
        "only {} of {GEFS_MEMBERS} GEFS members available for {}",
        members.len(),
        field.label()
    );
    Ok(EnsembleRun {
        members,
        run,
        fcst_hour: fh,
    })
}

/// One lead of an ensemble plume at a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlumePoint {
    pub valid: DateTime<Utc>,
    /// Ensemble mean, in the field's native units. `None` where that lead could not be fetched.
    pub mean: Option<f32>,
    /// Ensemble standard deviation, in native units. `None` alongside a missing mean.
    pub spread: Option<f32>,
}

impl PlumePoint {
    /// `mean ± k` standard deviations, or `None` if either half is missing. About two thirds of
    /// members lie within one deviation of the mean when the spread is roughly bell-shaped, which
    /// is how a plume is meant to be read: a range of plausible outcomes, not a hard bound.
    pub fn band(&self, k: f32) -> Option<(f32, f32)> {
        let (m, s) = (self.mean?, self.spread?);
        (m.is_finite() && s.is_finite() && s >= 0.0).then_some((m - k * s, m + k * s))
    }
}

/// The GEFS mean and spread of `field` at `(lon, lat)` for each forecast hour in `hours`, all from
/// one cycle: the newest that has posted the first hour, then pinned so the plume describes one
/// run rather than a patchwork. A lead that fails to arrive is a gap (`None`) in its slot, not an
/// error for the whole plume. The location must be inside the grid, which for GEFS is the globe.
pub async fn fetch_gefs_plume(
    http: &reqwest::Client,
    field: EnsembleField,
    lon: f64,
    lat: f64,
    hours: &[u16],
) -> anyhow::Result<Vec<PlumePoint>> {
    let key = field.gefs_key();
    // Leads the field does not exist at (F+0 for an accumulation) are dropped, not fetched to fail.
    let hours: Vec<u16> = hours
        .iter()
        .copied()
        .filter(|h| field.valid_lead(*h))
        .collect();
    let hours = hours.as_slice();
    let Some((&first, _)) = hours.split_first() else {
        return Ok(Vec::new());
    };
    // Find the cycle by asking for the first hour's mean, walking back like every other GEFS fetch.
    let mut run = None;
    let mut last_err = None;
    for candidate in crate::global::gefs_cycles(Utc::now(), 5) {
        match crate::global::fetch_gefs_summary(http, key, candidate, first, false).await {
            Ok(_) => {
                run = Some(candidate);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let run = run.ok_or_else(|| {
        last_err.unwrap_or_else(|| anyhow::anyhow!("no GEFS cycle found for the plume"))
    })?;
    let mut out = Vec::with_capacity(hours.len());
    // Small batches, as for the members: this is 2 requests per lead.
    for batch in hours.chunks(6) {
        let got = futures_util::future::join_all(batch.iter().map(|&fh| async move {
            let (mean, spread) = futures_util::future::join(
                crate::global::fetch_gefs_summary(http, key, run, fh, false),
                crate::global::fetch_gefs_summary(http, key, run, fh, true),
            )
            .await;
            let valid = run + chrono::Duration::hours(i64::from(fh));
            match (mean, spread) {
                (Ok(m), Ok(s)) => PlumePoint {
                    valid,
                    mean: m.sample_bilinear(lon, lat),
                    spread: s.sample_bilinear(lon, lat),
                },
                _ => PlumePoint {
                    valid,
                    mean: None,
                    spread: None,
                },
            }
        }))
        .await;
        out.extend(got);
    }
    anyhow::ensure!(
        out.iter().any(|p| p.mean.is_some()),
        "no GEFS mean/spread arrived for {}",
        field.label()
    );
    Ok(out)
}

/// The fewest members that still count as an ensemble.
pub const MIN_MEMBERS: usize = 10;

fn same_lattice(a: &MrmsField, b: &MrmsField) -> bool {
    const EPS: f64 = 1e-6;
    a.nx == b.nx
        && a.ny == b.ny
        && a.time == b.time
        && (a.lon_west - b.lon_west).abs() < EPS
        && (a.lon_east - b.lon_east).abs() < EPS
        && (a.lat_north - b.lat_north).abs() < EPS
        && (a.lat_south - b.lat_south).abs() < EPS
}

/// Reduce `members` to one grid of `stat`.
///
/// Fails if there are no members or they do not share a lattice and valid time — a statistic
/// across grids that are not the same instant or the same cells would be meaningless.
pub fn combine(members: &[MrmsField], stat: Statistic) -> anyhow::Result<MrmsField> {
    let first = members
        .first()
        .ok_or_else(|| anyhow::anyhow!("no ensemble members"))?;
    anyhow::ensure!(
        members.iter().all(|m| same_lattice(first, m)),
        "ensemble members are not on one lattice and valid time"
    );
    let need = (members.len() / 2).max(2);
    let cells = first.nx * first.ny;
    let mut values = Vec::with_capacity(cells);
    let mut scratch: Vec<f32> = Vec::with_capacity(members.len());
    for i in 0..cells {
        scratch.clear();
        scratch.extend(
            members
                .iter()
                .map(|m| m.values[i])
                .filter(|v| v.is_finite()),
        );
        values.push(if scratch.len() < need {
            f32::NAN
        } else {
            reduce(&mut scratch, stat)
        });
    }
    Ok(MrmsField {
        values,
        nx: first.nx,
        ny: first.ny,
        lon_west: first.lon_west,
        lon_east: first.lon_east,
        lat_north: first.lat_north,
        lat_south: first.lat_south,
        time: first.time,
    })
}

/// `v` is non-empty and finite. Sorts in place for the order statistics.
fn reduce(v: &mut [f32], stat: Statistic) -> f32 {
    let n = v.len() as f32;
    match stat {
        Statistic::Mean => v.iter().sum::<f32>() / n,
        Statistic::Spread => {
            let mean = v.iter().sum::<f32>() / n;
            let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / (n - 1.0);
            var.sqrt()
        }
        Statistic::Min => v.iter().copied().fold(f32::INFINITY, f32::min),
        Statistic::Max => v.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        Statistic::Percentile(p) => {
            v.sort_by(f32::total_cmp);
            let rank = f32::from(p.min(100)) / 100.0 * (n - 1.0);
            let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
            v[lo] + (v[hi] - v[lo]) * (rank - lo as f32)
        }
        Statistic::ProbabilityAbove(t) => v.iter().filter(|&&x| x > t).count() as f32 / n * 100.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn grid(values: Vec<f32>) -> MrmsField {
        MrmsField {
            values,
            nx: 2,
            ny: 1,
            lon_west: -100.0,
            lon_east: -99.0,
            lat_north: 40.0,
            lat_south: 40.0,
            time: Utc.with_ymd_and_hms(2026, 9, 20, 6, 0, 0).unwrap(),
        }
    }

    fn members(rows: &[[f32; 2]]) -> Vec<MrmsField> {
        rows.iter().map(|r| grid(r.to_vec())).collect()
    }

    fn at(f: &MrmsField, i: usize) -> f32 {
        f.values[i]
    }

    #[test]
    fn mean_min_max_are_per_cell() {
        let m = members(&[[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
        assert_eq!(at(&combine(&m, Statistic::Mean).unwrap(), 0), 2.0);
        assert_eq!(at(&combine(&m, Statistic::Mean).unwrap(), 1), 20.0);
        assert_eq!(at(&combine(&m, Statistic::Min).unwrap(), 1), 10.0);
        assert_eq!(at(&combine(&m, Statistic::Max).unwrap(), 0), 3.0);
    }

    #[test]
    fn spread_is_the_sample_standard_deviation() {
        // 1, 2, 3 → mean 2, sample variance 1.
        let m = members(&[[1.0, 5.0], [2.0, 5.0], [3.0, 5.0]]);
        let s = combine(&m, Statistic::Spread).unwrap();
        assert!((at(&s, 0) - 1.0).abs() < 1e-6);
        // Identical members: no disagreement.
        assert_eq!(at(&s, 1), 0.0);
    }

    #[test]
    fn percentiles_interpolate_between_ranked_members() {
        let m = members(&[[4.0, 0.0], [1.0, 0.0], [3.0, 0.0], [2.0, 0.0]]);
        let p = |q| at(&combine(&m, Statistic::Percentile(q)).unwrap(), 0);
        assert_eq!(p(0), 1.0);
        assert_eq!(p(100), 4.0);
        assert!((p(50) - 2.5).abs() < 1e-6);
        // Rank 0.25 * 3 = 0.75 → between 1 and 2.
        assert!((p(25) - 1.75).abs() < 1e-6);
    }

    #[test]
    fn probability_counts_members_strictly_above_the_threshold() {
        let m = members(&[[0.0, 1.0], [5.0, 1.0], [10.0, 1.0], [10.0, 1.0]]);
        let f = combine(&m, Statistic::ProbabilityAbove(5.0)).unwrap();
        // 10 and 10 are above 5; 5 itself is not.
        assert_eq!(at(&f, 0), 50.0);
        assert_eq!(at(&f, 1), 0.0);
        let all = combine(&m, Statistic::ProbabilityAbove(-1.0)).unwrap();
        assert_eq!(at(&all, 0), 100.0);
    }

    #[test]
    fn missing_members_are_skipped_but_a_mostly_missing_cell_is_missing() {
        let m = members(&[
            [1.0, f32::NAN],
            [3.0, f32::NAN],
            [f32::NAN, f32::NAN],
            [2.0, 9.0],
        ]);
        let f = combine(&m, Statistic::Mean).unwrap();
        // Three of four present: the mean is over those three.
        assert_eq!(at(&f, 0), 2.0);
        // One of four present is below half: no statistic rather than one member's word.
        assert!(at(&f, 1).is_nan());
    }

    #[test]
    fn display_conversions_round_trip_and_spread_ignores_the_offset() {
        for f in EnsembleField::ALL {
            let t = f.default_threshold();
            assert!((f.from_display(f.to_display(t)) - t).abs() < t.abs() * 1e-5 + 1e-3);
        }
        // 273.15 K is the freezing point; a 2 K spread is 2 °C, not -271.15.
        assert!((EnsembleField::Temp2m.to_display(273.15)).abs() < 1e-4);
        assert_eq!(EnsembleField::Temp2m.spread_to_display(2.0), 2.0);
        assert!((EnsembleField::Mslp.spread_to_display(500.0) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn an_accumulation_exists_only_at_leads_that_close_a_window() {
        let qpf = EnsembleField::Precip6h;
        assert!(!qpf.valid_lead(0), "nothing has accumulated at F+0");
        assert!(!qpf.valid_lead(3) && !qpf.valid_lead(9) && !qpf.valid_lead(7));
        assert!(qpf.valid_lead(6) && qpf.valid_lead(24) && qpf.valid_lead(240));
        // A lead chosen on the three-hourly grid rounds up to the next window.
        assert_eq!(qpf.snap_lead(0), 6);
        assert_eq!(qpf.snap_lead(3), 6);
        assert_eq!(qpf.snap_lead(6), 6);
        assert_eq!(qpf.snap_lead(9), 12);
        for f in EnsembleField::ALL {
            let snapped = f.snap_lead(9);
            assert!(f.valid_lead(snapped), "{f:?}: snapped to {snapped}");
            // Snapping never goes backwards and never skips a lead that was already valid.
            assert!(snapped >= 9 || !f.valid_lead(9));
            if f.valid_lead(9) {
                assert_eq!(snapped, 9, "{f:?} snapshots keep the lead as chosen");
            }
        }
    }

    #[test]
    fn a_plume_band_needs_both_halves_and_a_sane_spread() {
        use chrono::TimeZone;
        let valid = Utc.with_ymd_and_hms(2026, 9, 21, 0, 0, 0).unwrap();
        let p = |mean, spread| PlumePoint {
            valid,
            mean,
            spread,
        };
        assert_eq!(p(Some(10.0), Some(2.0)).band(1.0), Some((8.0, 12.0)));
        assert_eq!(p(Some(10.0), Some(2.0)).band(2.0), Some((6.0, 14.0)));
        // A missing half, a NaN or a negative spread is no band rather than a wrong one.
        assert_eq!(p(None, Some(2.0)).band(1.0), None);
        assert_eq!(p(Some(10.0), None).band(1.0), None);
        assert_eq!(p(Some(f32::NAN), Some(2.0)).band(1.0), None);
        assert_eq!(p(Some(10.0), Some(-1.0)).band(1.0), None);
    }

    #[test]
    fn statistic_specs_parse() {
        let f = EnsembleField::Cape;
        assert_eq!(Statistic::parse("mean", f), Some(Statistic::Mean));
        assert_eq!(
            Statistic::parse(" P90 ", f),
            Some(Statistic::Percentile(90))
        );
        assert_eq!(
            Statistic::parse("prob", f),
            Some(Statistic::ProbabilityAbove(1_000.0))
        );
        assert_eq!(
            Statistic::parse("prob:2500", f),
            Some(Statistic::ProbabilityAbove(2500.0))
        );
        assert_eq!(Statistic::parse("p101", f), None);
        assert_eq!(Statistic::parse("median", f), None);
        assert_eq!(Statistic::parse("prob:abc", f), None);
    }

    #[test]
    fn mismatched_lattices_or_times_are_rejected() {
        let mut a = members(&[[1.0, 1.0], [2.0, 2.0]]);
        a[1].time += chrono::Duration::hours(1);
        assert!(combine(&a, Statistic::Mean).is_err());
        let mut b = members(&[[1.0, 1.0], [2.0, 2.0]]);
        b[1].lon_east = -98.0;
        assert!(combine(&b, Statistic::Mean).is_err());
        assert!(combine(&[], Statistic::Mean).is_err());
    }

    #[test]
    fn every_field_has_a_unique_slug_and_a_sane_default_threshold() {
        let mut slugs: Vec<_> = EnsembleField::ALL.iter().map(|f| f.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), EnsembleField::ALL.len());
        for f in EnsembleField::ALL {
            assert_eq!(EnsembleField::from_slug(f.slug()), Some(f));
            assert!(f.default_threshold().is_finite());
        }
    }

    /// Live GEFS QPF: the accumulation is a real, non-negative field, the exceedance probability is
    /// a percentage, and a wetter threshold is never likelier than a drier one. Network test.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_gefs_qpf_probability_is_coherent() {
        let http = reqwest::Client::new();
        let run = fetch_gefs(&http, EnsembleField::Precip6h, 24)
            .await
            .expect("GEFS QPF ensemble");
        assert!(run.members.len() >= MIN_MEMBERS);
        let mean = combine(&run.members, Statistic::Mean).unwrap();
        let light = combine(&run.members, Statistic::ProbabilityAbove(2.5)).unwrap();
        let heavy = combine(&run.members, Statistic::ProbabilityAbove(25.0)).unwrap();
        let (mut wet, mut checked) = (0usize, 0usize);
        for i in 0..mean.values.len() {
            let (m, l, h) = (mean.values[i], light.values[i], heavy.values[i]);
            if m.is_finite() && l.is_finite() && h.is_finite() {
                assert!(m >= -1e-3, "negative accumulation {m}");
                assert!((0.0..=100.0).contains(&l) && (0.0..=100.0).contains(&h));
                assert!(h <= l + 1e-3, "heavier rain cannot be likelier: {h} vs {l}");
                wet += usize::from(l > 0.0);
                checked += 1;
            }
        }
        assert!(checked > 1000, "only {checked} cells compared");
        println!("QPF f024: {wet} of {checked} cells have some member above 2.5 mm");
        // An accumulation does not exist at hour zero, and asking for it is an error up front.
        assert!(fetch_gefs(&http, EnsembleField::Precip6h, 0).await.is_err());
    }

    /// Live GEFS plume: mean and spread at Oklahoma City for three days, all from one run, with
    /// spread that is non-negative and generally larger late than early. Network test.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_gefs_plume_is_coherent() {
        let http = reqwest::Client::new();
        let hours: Vec<u16> = (0..=72).step_by(6).collect();
        let plume = fetch_gefs_plume(&http, EnsembleField::Temp2m, -97.5, 35.5, &hours)
            .await
            .expect("GEFS plume");
        assert_eq!(plume.len(), hours.len());
        let have: Vec<_> = plume.iter().filter(|p| p.band(1.0).is_some()).collect();
        assert!(
            have.len() >= hours.len() - 2,
            "only {} leads arrived",
            have.len()
        );
        for p in &have {
            let (m, s) = (p.mean.unwrap(), p.spread.unwrap());
            assert!((230.0..330.0).contains(&m), "{m} K");
            assert!((0.0..25.0).contains(&s), "{s} K spread");
        }
        // One run: valid times step evenly.
        assert!(plume
            .windows(2)
            .all(|w| w[1].valid - w[0].valid == chrono::Duration::hours(6)));
        let early: f32 = have[..3].iter().map(|p| p.spread.unwrap()).sum();
        let late: f32 = have[have.len() - 3..]
            .iter()
            .map(|p| p.spread.unwrap())
            .sum();
        println!("spread early {early:.2} late {late:.2}");
        assert!(late > early, "uncertainty should grow with lead");
    }

    /// Live GEFS: every member arrives on one lattice and the statistics are ordered. Network
    /// test, run explicitly with `--ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_gefs_temperature_ensemble_is_coherent() {
        let http = reqwest::Client::new();
        let run = fetch_gefs(&http, EnsembleField::Temp2m, 24)
            .await
            .expect("GEFS ensemble");
        assert!(run.members.len() >= MIN_MEMBERS);
        let (lo, mean, hi) = (
            combine(&run.members, Statistic::Min).unwrap(),
            combine(&run.members, Statistic::Mean).unwrap(),
            combine(&run.members, Statistic::Max).unwrap(),
        );
        let mut checked = 0;
        for i in 0..mean.values.len() {
            let (a, b, c) = (lo.values[i], mean.values[i], hi.values[i]);
            if a.is_finite() && b.is_finite() && c.is_finite() {
                assert!(a <= b + 1e-3 && b <= c + 1e-3, "{a} {b} {c}");
                checked += 1;
            }
        }
        assert!(checked > 1000, "only {checked} cells compared");
    }
}
