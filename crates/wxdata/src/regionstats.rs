//! Region statistics for one radar tilt: every gate inside a lon/lat box, with each moment read at
//! that same gate, so they can be compared gate by gate — the data behind the multi-moment scatter
//! plots, histograms and CSV export (ROADMAP_NEW C4).
//!
//! The moments of one tilt are binned separately and do not share a grid: reflectivity usually
//! reaches further than velocity, and the gate spacing can differ. The first sweep given is the
//! reference — its gates are the rows — and every other moment is read at the same azimuth and
//! range from its own sweep, so a row is one place in the sky across all the moments.
//!
//! Positions use a flat-earth projection about the radar with range taken along the ground (the
//! gate's slant range brought down through the beam model, which matters on the upper tilts: at
//! 15 deg the slant range runs 3% past the ground under it).
//!
//! A storm cell's core reads every tilt ([`gather_tilt`] per tilt, then [`stack`]): each row keeps
//! the tilt it came from, so the whole cell can be summarised, or split back tilt by tilt.

use crate::level2::{BinnedSweep, Moment};
use crate::tds::decode;

/// One gate of the reference sweep inside the box, and each moment's value there.
#[derive(Debug, Clone, PartialEq)]
pub struct GateRow {
    pub lon: f64,
    pub lat: f64,
    /// Slant range, as the radar measures it.
    pub range_km: f32,
    pub az_deg: f32,
    /// The tilt the gate is on.
    pub elevation_deg: f32,
    /// One entry per [`RegionSamples::moments`], in the same order; `None` where that moment has
    /// no data at this gate (below its threshold, range-folded, or past its sweep's reach).
    pub values: Vec<Option<f32>>,
}

/// Every reference gate inside a box, sampled in every moment given: on one tilt, or on several
/// stacked ([`stack`]), each row naming its own.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionSamples {
    pub moments: Vec<Moment>,
    pub rows: Vec<GateRow>,
    /// The tilt, or the lowest of those stacked.
    pub elevation_deg: f32,
    /// `(west, south, east, north)` in degrees.
    pub bbox: (f64, f64, f64, f64),
}

/// The spread of one moment's values in the region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub min: f32,
    pub p10: f32,
    pub median: f32,
    pub p90: f32,
    pub max: f32,
    pub mean: f32,
}

/// Equal-width bins from `lo` to `hi`, with `counts[i]` values in bin `i`.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub lo: f32,
    pub hi: f32,
    pub counts: Vec<usize>,
}

impl Histogram {
    /// The width of one bin.
    pub fn bin_width(&self) -> f32 {
        (self.hi - self.lo) / self.counts.len().max(1) as f32
    }
}

/// `(west, south, east, north)` from any two opposite corners, in either order.
pub fn bbox_of(a: [f64; 2], b: [f64; 2]) -> (f64, f64, f64, f64) {
    (
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    )
}

/// The value of `sweep` at `az_deg` / `range_km`, or `None` where it has none.
fn sample(sweep: &BinnedSweep, az_deg: f64, range_km: f32) -> Option<f32> {
    if sweep.az_bins == 0 || sweep.gate_count == 0 || sweep.gate_interval_km <= 0.0 {
        return None;
    }
    let bin = ((az_deg.rem_euclid(360.0) / 360.0 * sweep.az_bins as f64) as usize) % sweep.az_bins;
    let gate = ((range_km - sweep.first_gate_km) / sweep.gate_interval_km).round();
    if gate < 0.0 || gate as usize >= sweep.gate_count {
        return None;
    }
    decode(sweep, sweep.data[bin * sweep.gate_count + gate as usize])
}

/// Every gate of `sweeps[0]` inside `bbox` (`(west, south, east, north)`), with each sweep's value
/// read at the same place. `None` when there is no reference sweep, or no gate falls in the box.
pub fn gather(sweeps: &[BinnedSweep], bbox: (f64, f64, f64, f64)) -> Option<RegionSamples> {
    let moments: Vec<Moment> = sweeps.iter().map(|s| s.moment).collect();
    let present: Vec<Option<&BinnedSweep>> = sweeps.iter().map(Some).collect();
    gather_tilt(&moments, &present, bbox)
}

/// [`gather`] on one tilt with a fixed list of moments: `sweeps[i]` is `moments[i]` on this tilt,
/// `None` where the tilt does not carry it (that column is then blank), so tilts gathered alike
/// can be [`stack`]ed. The first sweep present is the reference.
pub fn gather_tilt(
    moments: &[Moment],
    sweeps: &[Option<&BinnedSweep>],
    bbox: (f64, f64, f64, f64),
) -> Option<RegionSamples> {
    let reference = sweeps.iter().flatten().next().copied()?;
    if reference.az_bins == 0 || reference.gate_count == 0 {
        return None;
    }
    let elevation = f64::from(reference.elevation_deg);
    let (west, south, east, north) = bbox;
    let (rlon, rlat) = (
        f64::from(reference.radar_lon),
        f64::from(reference.radar_lat),
    );
    let km_per_deg_lon = 111.32 * rlat.to_radians().cos().max(0.01);
    const KM_PER_DEG_LAT: f64 = 110.57;
    let mut rows = Vec::new();
    for bin in 0..reference.az_bins {
        let az_deg = (bin as f64 + 0.5) * 360.0 / reference.az_bins as f64;
        let (sin, cos) = az_deg.to_radians().sin_cos();
        for gate in 0..reference.gate_count {
            let range_km = reference.first_gate_km + gate as f32 * reference.gate_interval_km;
            let r = crate::xsection::ground_from_slant_km(f64::from(range_km), elevation);
            let lon = rlon + r * sin / km_per_deg_lon;
            let lat = rlat + r * cos / KM_PER_DEG_LAT;
            if lon < west || lon > east || lat < south || lat > north {
                continue;
            }
            let values: Vec<Option<f32>> = sweeps
                .iter()
                .map(|s| s.and_then(|s| sample(s, az_deg, range_km)))
                .collect();
            // A gate no moment has anything at is empty sky, not a sample.
            if values.iter().all(Option::is_none) {
                continue;
            }
            rows.push(GateRow {
                lon,
                lat,
                range_km,
                az_deg: az_deg as f32,
                elevation_deg: reference.elevation_deg,
                values,
            });
        }
    }
    (!rows.is_empty()).then(|| RegionSamples {
        moments: moments.to_vec(),
        rows,
        elevation_deg: reference.elevation_deg,
        bbox,
    })
}

/// Tilts of one place as one sample, lowest tilt first, every row keeping its own tilt. The parts
/// must be gathered with the same moments ([`gather_tilt`]); `None` when there are none.
pub fn stack(mut parts: Vec<RegionSamples>) -> Option<RegionSamples> {
    parts.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    let mut it = parts.into_iter();
    let mut out = it.next()?;
    for p in it {
        debug_assert_eq!(p.moments, out.moments, "stacked tilts share their moments");
        out.rows.extend(p.rows);
    }
    Some(out)
}

/// The value at quantile `q` (0..1) of an ascending slice, by nearest rank.
fn quantile(sorted: &[f32], q: f32) -> f32 {
    let i = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

impl RegionSamples {
    /// The tilts the rows are on, lowest first.
    pub fn tilts(&self) -> Vec<f32> {
        let mut t: Vec<f32> = self.rows.iter().map(|r| r.elevation_deg).collect();
        t.sort_by(f32::total_cmp);
        t.dedup();
        t
    }

    /// The rows of one tilt, as a sample of their own; `None` when no row is on it.
    pub fn tilt(&self, elevation_deg: f32) -> Option<RegionSamples> {
        let rows: Vec<GateRow> = self
            .rows
            .iter()
            .filter(|r| r.elevation_deg == elevation_deg)
            .cloned()
            .collect();
        (!rows.is_empty()).then(|| RegionSamples {
            moments: self.moments.clone(),
            rows,
            elevation_deg,
            bbox: self.bbox,
        })
    }

    /// The position of `moment` among [`Self::moments`].
    pub fn index_of(&self, moment: Moment) -> Option<usize> {
        self.moments.iter().position(|m| *m == moment)
    }

    /// Every value moment `i` has in the region.
    pub fn values(&self, i: usize) -> Vec<f32> {
        self.rows
            .iter()
            .filter_map(|r| r.values.get(i).copied().flatten())
            .collect()
    }

    /// How moment `i` is spread across the region, or `None` when it has no values there.
    pub fn summary(&self, i: usize) -> Option<Summary> {
        let mut v = self.values(i);
        if v.is_empty() {
            return None;
        }
        v.sort_by(f32::total_cmp);
        let mean = v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64;
        Some(Summary {
            n: v.len(),
            min: v[0],
            p10: quantile(&v, 0.10),
            median: quantile(&v, 0.50),
            p90: quantile(&v, 0.90),
            max: v[v.len() - 1],
            mean: mean as f32,
        })
    }

    /// Moment `i`'s values in `bins` equal bins from its minimum to its maximum, or `None` when it
    /// has no values. A region holding one value is one full bin.
    pub fn histogram(&self, i: usize, bins: usize) -> Option<Histogram> {
        let v = self.values(i);
        let s = self.summary(i)?;
        let bins = bins.max(1);
        let (lo, hi) = if s.max > s.min {
            (s.min, s.max)
        } else {
            (s.min - 0.5, s.min + 0.5)
        };
        let mut counts = vec![0usize; bins];
        for x in v {
            let b = (((x - lo) / (hi - lo)) * bins as f32) as usize;
            counts[b.min(bins - 1)] += 1;
        }
        Some(Histogram { lo, hi, counts })
    }

    /// `(moment a, moment b)` at every gate where both have a value — one scatter-plot point each.
    pub fn pairs(&self, a: usize, b: usize) -> Vec<(f32, f32)> {
        self.rows
            .iter()
            .filter_map(|r| Some((r.values.get(a).copied()??, r.values.get(b).copied()??)))
            .collect()
    }

    /// Pearson correlation of moments `a` and `b` over the gates both have, or `None` with fewer
    /// than three such gates or no spread in either.
    pub fn correlation(&self, a: usize, b: usize) -> Option<f32> {
        let p = self.pairs(a, b);
        if p.len() < 3 {
            return None;
        }
        let n = p.len() as f64;
        let (mx, my) = p.iter().fold((0.0, 0.0), |(x, y), (a, b)| {
            (x + f64::from(*a) / n, y + f64::from(*b) / n)
        });
        let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
        for (a, b) in &p {
            let (dx, dy) = (f64::from(*a) - mx, f64::from(*b) - my);
            sxy += dx * dy;
            sxx += dx * dx;
            syy += dy * dy;
        }
        // A constant is its mean to rounding, not exactly: "no spread" is a variance under this.
        const NO_SPREAD: f64 = 1e-9;
        (sxx / n > NO_SPREAD && syy / n > NO_SPREAD).then(|| (sxy / (sxx * syy).sqrt()) as f32)
    }

    /// Every gate as a CSV row — position, then each moment in [`Self::moments`] order, blank where
    /// a moment has no value — under a header naming the moments by their product codes.
    pub fn to_csv(&self) -> String {
        let mut out = String::from("lat,lon,range_km,azimuth_deg,elevation_deg");
        for m in &self.moments {
            out.push(',');
            out.push_str(m.short_name());
        }
        out.push('\n');
        for r in &self.rows {
            out.push_str(&format!(
                "{:.5},{:.5},{:.3},{:.2},{:.2}",
                r.lat, r.lon, r.range_km, r.az_deg, r.elevation_deg
            ));
            for v in &r.values {
                out.push(',');
                if let Some(v) = v {
                    out.push_str(&format!("{v:.3}"));
                }
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sweep of `moment` holding `value` everywhere, or a function of (azimuth bin, gate).
    fn sweep(
        moment: Moment,
        gates: usize,
        interval: f32,
        f: impl Fn(usize, usize) -> f32,
    ) -> BinnedSweep {
        let az_bins = 360;
        let (lo, hi) = moment.value_range();
        let idx = |v: f32| {
            (2.0 + (v - lo) / (hi - lo) * 253.0)
                .round()
                .clamp(2.0, 255.0) as u8
        };
        let mut data = vec![0u8; az_bins * gates];
        for a in 0..az_bins {
            for g in 0..gates {
                data[a * gates + g] = idx(f(a, g));
            }
        }
        BinnedSweep {
            moment,
            az_bins,
            gate_count: gates,
            data,
            first_gate_km: 0.0,
            gate_interval_km: interval,
            radar_lat: 35.0,
            radar_lon: -97.5,
            elevation_deg: 0.5,
            value_min: lo,
            value_max: hi,
            ..Default::default()
        }
    }

    /// A box due north of the radar, 10-20 km out and about 4 km wide.
    fn north_box() -> (f64, f64, f64, f64) {
        bbox_of(
            [-97.522, 35.0 + 10.0 / 110.57],
            [-97.478, 35.0 + 20.0 / 110.57],
        )
    }

    #[test]
    fn a_box_holds_only_the_gates_inside_it_and_reads_every_moment_at_each() {
        // Reflectivity rises with range; velocity is only 100 km deep and on a coarser grid.
        let z = sweep(Moment::Reflectivity, 400, 0.25, |_, g| {
            10.0 + g as f32 * 0.1
        });
        let v = sweep(Moment::Velocity, 100, 1.0, |_, g| g as f32 * 0.5 - 20.0);
        let s = gather(&[z, v], north_box()).unwrap();
        assert_eq!(s.moments, vec![Moment::Reflectivity, Moment::Velocity]);
        assert!(!s.rows.is_empty());
        let (w, so, e, n) = north_box();
        for r in &s.rows {
            assert!(r.lon >= w && r.lon <= e && r.lat >= so && r.lat <= n);
            assert!((10.0..=20.0).contains(&r.range_km), "{}", r.range_km);
            // Both moments read at the row's own range: REF 10 + 0.1/gate at 0.25 km, VEL -20 +
            // 0.5/gate at 1 km, so each is a known function of range (to its bin quantisation).
            let want_z = 10.0 + r.range_km / 0.25 * 0.1;
            let want_v = (r.range_km).round() * 0.5 - 20.0;
            assert!((r.values[0].unwrap() - want_z).abs() < 0.3, "{:?}", r);
            assert!((r.values[1].unwrap() - want_v).abs() < 0.6, "{:?}", r);
        }
        assert!(gather(&[], north_box()).is_none());
        // A box with no gates in it (far past the sweep) is nothing.
        assert!(gather(&s_ref(), (-90.0, 40.0, -89.0, 41.0)).is_none());
    }

    fn s_ref() -> Vec<BinnedSweep> {
        vec![sweep(Moment::Reflectivity, 400, 0.25, |_, _| 30.0)]
    }

    #[test]
    fn a_moment_missing_past_its_reach_is_blank_not_zero() {
        let z = sweep(Moment::Reflectivity, 400, 0.25, |_, _| 30.0);
        let v = sweep(Moment::Velocity, 60, 0.25, |_, _| 5.0); // 15 km deep only
        let s = gather(&[z, v], north_box()).unwrap();
        let far: Vec<&GateRow> = s.rows.iter().filter(|r| r.range_km > 16.0).collect();
        assert!(!far.is_empty());
        assert!(far.iter().all(|r| r.values[1].is_none()));
        let csv = s.to_csv();
        assert!(
            csv.starts_with("lat,lon,range_km,azimuth_deg,elevation_deg,REF,VEL\n"),
            "{csv}"
        );
        assert!(csv.lines().any(|l| l.ends_with(',')), "a blank VEL cell");
        assert_eq!(csv.lines().count(), s.rows.len() + 1);
    }

    #[test]
    fn summary_histogram_pairs_and_correlation() {
        // ZDR a straight function of REF through the box, so r = 1; CC falling as REF rises.
        let z = sweep(Moment::Reflectivity, 400, 0.25, |_, g| {
            20.0 + g as f32 * 0.1
        });
        let zdr = sweep(Moment::DifferentialReflectivity, 400, 0.25, |_, g| {
            g as f32 * 0.01
        });
        let cc = sweep(Moment::CorrelationCoefficient, 400, 0.25, |_, g| {
            1.0 - g as f32 * 0.001
        });
        let s = gather(&[z, zdr, cc], north_box()).unwrap();
        let sum = s.summary(0).unwrap();
        assert_eq!(sum.n, s.rows.len());
        assert!(sum.min <= sum.p10 && sum.p10 <= sum.median && sum.median <= sum.p90);
        assert!(sum.p90 <= sum.max);
        assert!(
            (sum.min - 24.0).abs() < 0.3 && (sum.max - 28.0).abs() < 0.3,
            "{sum:?}"
        );

        let h = s.histogram(0, 8).unwrap();
        assert_eq!(h.counts.len(), 8);
        assert_eq!(h.counts.iter().sum::<usize>(), sum.n);
        assert!((h.lo - sum.min).abs() < 1e-6 && (h.hi - sum.max).abs() < 1e-6);

        assert_eq!(s.pairs(0, 1).len(), s.rows.len());
        let r = s.correlation(0, 1).unwrap();
        assert!(r > 0.98, "{r}");
        let r = s.correlation(0, 2).unwrap();
        assert!(r < -0.98, "{r}");
        assert_eq!(s.index_of(Moment::CorrelationCoefficient), Some(2));
        assert_eq!(s.index_of(Moment::Velocity), None);
    }

    /// A cell read tilt by tilt: each tilt gathered with the same moments (a tilt without velocity
    /// leaves that column blank), stacked lowest first with every row keeping its tilt, and split
    /// back. Positions are over the ground under the gate, which on a 15 deg tilt is 3% short of
    /// its slant range.
    #[test]
    fn a_cell_reads_every_tilt_and_splits_back_tilt_by_tilt() {
        let moments = [Moment::Reflectivity, Moment::Velocity];
        let low_z = sweep(Moment::Reflectivity, 400, 0.25, |_, _| 30.0);
        let low_v = sweep(Moment::Velocity, 400, 0.25, |_, _| 12.0);
        let mut high_z = sweep(Moment::Reflectivity, 400, 0.25, |_, _| 50.0);
        high_z.elevation_deg = 15.0;
        let low = gather_tilt(&moments, &[Some(&low_z), Some(&low_v)], north_box()).unwrap();
        let high = gather_tilt(&moments, &[Some(&high_z), None], north_box()).unwrap();
        let cell = stack(vec![high, low]).unwrap();
        assert_eq!(cell.tilts(), [0.5, 15.0]);
        assert_eq!(cell.elevation_deg, 0.5, "the lowest");
        assert_eq!(cell.moments, moments);
        let top = cell.tilt(15.0).unwrap();
        assert!(top.rows.iter().all(|r| r.elevation_deg == 15.0));
        assert!(
            top.rows.iter().all(|r| r.values[1].is_none()),
            "no velocity up there"
        );
        assert!((top.summary(0).unwrap().max - 50.0).abs() < 0.3);
        let base = cell.tilt(0.5).unwrap();
        assert_eq!(base.rows.len() + top.rows.len(), cell.rows.len());
        assert!((base.summary(1).unwrap().max - 12.0).abs() < 0.6);
        assert!(cell.tilt(3.0).is_none());
        // Due north: the row's distance north is the ground under the gate, not its slant range.
        for r in &top.rows {
            let north_km = (r.lat - 35.0) * 110.57;
            let ground = crate::xsection::ground_from_slant_km(f64::from(r.range_km), 15.0);
            assert!((north_km - ground).abs() < 0.3, "{north_km} vs {ground}");
            assert!(ground < f64::from(r.range_km) * 0.975);
        }
        assert!(stack(Vec::new()).is_none());
        assert!(gather_tilt(&moments, &[None, None], north_box()).is_none());
    }

    #[test]
    fn one_value_everywhere_is_one_full_bin_and_no_correlation() {
        let s = gather(
            &[
                sweep(Moment::Reflectivity, 400, 0.25, |_, _| 30.0),
                sweep(Moment::DifferentialReflectivity, 400, 0.25, |_, g| {
                    g as f32 * 0.01
                }),
            ],
            north_box(),
        )
        .unwrap();
        let h = s.histogram(0, 10).unwrap();
        assert_eq!(h.counts.iter().filter(|c| **c > 0).count(), 1);
        assert_eq!(s.correlation(0, 1), None, "no spread in REF");
        assert_eq!(bbox_of([1.0, 4.0], [3.0, 2.0]), (1.0, 2.0, 3.0, 4.0));
    }
}
