//! Following an analyst-picked feature through a radar loop (ROADMAP_NEW R4, 1010.md V14).
//!
//! The analyst points at a mesocyclone, a reflectivity or hail core, or a bounded weak echo
//! region in one volume. [`track`] follows it through the loop's other volumes, forward and back:
//! from each fix it predicts the next one from the feature's own motion so far, searches around
//! the prediction for the strongest value of the feature's kind ([`rotation_peak`]: cyclonic LLSD
//! azimuthal shear; [`core_peak`]: reflectivity; [`hail_peak`]: MEHS; [`bwer_peak`]: a weak echo
//! walled in and capped by strong echo), and stops after two volumes in a row where nothing
//! strong enough is there.
//! A fix the analyst places by hand is kept as placed, and the track continues from it.
//!
//! Each fix carries a confidence from 0 to 1: how far the peak stands above the searched area
//! around it, how strong it is against the kind's floor, and how near it is to where the motion
//! predicted it. It says how clearly one feature was followed, not how likely a tornado is.

use crate::level2::BinnedSweep;
use chrono::{DateTime, Utc};

/// What is being followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FeatureKind {
    /// A rotation: the strongest cyclonic azimuthal shear (s⁻¹).
    Rotation,
    /// A reflectivity core: the strongest reflectivity (dBZ).
    Core,
    /// A hail core: the largest maximum expected hail size (MEHS, mm; Witt et al. 1998).
    Hail,
    /// A bounded weak echo region candidate: how far a mid-level weak echo sits below the strong
    /// echo walling it in and capping it (dB).
    Bwer,
}

impl FeatureKind {
    pub fn label(self) -> &'static str {
        match self {
            FeatureKind::Rotation => "rotation",
            FeatureKind::Core => "core",
            FeatureKind::Hail => "hail core",
            FeatureKind::Bwer => "BWER candidate",
        }
    }

    /// The unit the kind's value is in.
    pub fn unit(self) -> &'static str {
        match self {
            FeatureKind::Rotation => "s\u{207b}\u{b9}",
            FeatureKind::Core => "dBZ",
            FeatureKind::Hail => "mm",
            FeatureKind::Bwer => "dB",
        }
    }
}

/// The strongest value found around a point, where it was, and the searched area's median.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// `[lon, lat]`.
    pub ll: [f64; 2],
    pub value: f32,
    pub background: f32,
}

/// One volume's fix.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fix {
    pub time: DateTime<Utc>,
    /// `[lon, lat]`.
    pub ll: [f64; 2],
    /// The peak's value in the kind's unit; `None` for a fix placed by hand on a volume the
    /// sampler could not read.
    pub value: Option<f32>,
    pub confidence: f32,
    /// Placed by the analyst.
    pub manual: bool,
}

/// How a kind is followed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackParams {
    /// Search radius around a prediction, km.
    pub search_km: f64,
    /// How far a feature with no motion yet may have gone, m/s.
    pub max_speed_ms: f64,
    /// Below this the feature is lost at that volume.
    pub min_value: f32,
}

impl TrackParams {
    pub fn for_kind(kind: FeatureKind) -> Self {
        match kind {
            // 0.004 s⁻¹ is a weak mesocyclone in LLSD terms (MRMS AzShear's 0.004-0.006 band).
            FeatureKind::Rotation => TrackParams {
                search_km: 6.0,
                max_speed_ms: 35.0,
                min_value: 0.004,
            },
            FeatureKind::Core => TrackParams {
                search_km: 8.0,
                max_speed_ms: 35.0,
                min_value: 40.0,
            },
            // 13 mm is half-inch hail, well under the 19 mm POSH counts as severe: a core is
            // followed while it still grows hail, not only while it is severe.
            FeatureKind::Hail => TrackParams {
                search_km: 8.0,
                max_speed_ms: 35.0,
                min_value: 13.0,
            },
            // A hole 10 dB under the wall around it; a mature BWER is 20 dB and more.
            FeatureKind::Bwer => TrackParams {
                search_km: 6.0,
                max_speed_ms: 35.0,
                min_value: 10.0,
            },
        }
    }
}

const EARTH_KM: f64 = 6371.0;

fn dist_km(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dla = la2 - la1;
    let dlo = (b[0] - a[0]).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * EARTH_KM * h.sqrt().asin()
}

/// Bearing from `a` to `b`, degrees clockwise from north.
fn bearing_deg(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dlo = (b[0] - a[0]).to_radians();
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// The gates of a polar grid within `radius_km` of `ll`, as `(az, gate, lonlat)`: a window of
/// azimuths and ranges around the point, then the exact distance.
fn gates_near(
    radar: [f64; 2],
    az_bins: usize,
    gate_count: usize,
    first_gate_km: f32,
    gate_interval_km: f32,
    ll: [f64; 2],
    radius_km: f64,
) -> Vec<(usize, usize, [f64; 2])> {
    let range = dist_km(radar, ll);
    if az_bins == 0 || gate_count == 0 || gate_interval_km <= 0.0 {
        return Vec::new();
    }
    let az = bearing_deg(radar, ll);
    let half_deg = if range > radius_km {
        (radius_km / range).asin().to_degrees()
    } else {
        180.0
    };
    let bin_deg = 360.0 / az_bins as f64;
    let span = (half_deg / bin_deg).ceil() as i64 + 1;
    let center_bin = (az / bin_deg).floor() as i64;
    let g0 = (((range - radius_km) - f64::from(first_gate_km)) / f64::from(gate_interval_km))
        .floor()
        .max(0.0) as usize;
    let g1 = ((((range + radius_km) - f64::from(first_gate_km)) / f64::from(gate_interval_km))
        .ceil()
        .max(0.0) as usize)
        .min(gate_count.saturating_sub(1));
    let mut out = Vec::new();
    let bins: Vec<usize> = if span as usize * 2 + 1 >= az_bins {
        (0..az_bins).collect()
    } else {
        (-span..=span)
            .map(|k| (center_bin + k).rem_euclid(az_bins as i64) as usize)
            .collect()
    };
    for b in bins {
        let a = (b as f64 + 0.5) * bin_deg;
        for g in g0..=g1 {
            let r = f64::from(first_gate_km) + (g as f64 + 0.5) * f64::from(gate_interval_km);
            let p = crate::rotation::dest(radar[0], radar[1], a, r);
            let p = [p.0, p.1];
            if dist_km(p, ll) <= radius_km {
                out.push((b, g, p));
            }
        }
    }
    out
}

fn peak_of(values: Vec<(f32, [f64; 2])>) -> Option<Peak> {
    let (value, ll) = values.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0))?;
    let mut sorted: Vec<f32> = values.iter().map(|v| v.0).collect();
    sorted.sort_by(f32::total_cmp);
    Some(Peak {
        ll,
        value,
        background: sorted[sorted.len() / 2],
    })
}

/// The strongest reflectivity within `radius_km` of `ll` on a reflectivity sweep, and the
/// searched gates' median (gates without echo count as the sweep's floor).
pub fn core_peak(sweep: &BinnedSweep, ll: [f64; 2], radius_km: f64) -> Option<Peak> {
    let radar = [f64::from(sweep.radar_lon), f64::from(sweep.radar_lat)];
    let gates = gates_near(
        radar,
        sweep.az_bins,
        sweep.gate_count,
        sweep.first_gate_km,
        sweep.gate_interval_km,
        ll,
        radius_km,
    );
    let decode = |code: u8| {
        if code >= 2 {
            sweep.value_min + (code - 2) as f32 / 253.0 * (sweep.value_max - sweep.value_min)
        } else {
            sweep.value_min
        }
    };
    let values: Vec<(f32, [f64; 2])> = gates
        .into_iter()
        .filter_map(|(az, g, p)| {
            let code = *sweep.data.get(az * sweep.gate_count + g)?;
            Some((decode(code), p))
        })
        .collect();
    peak_of(values).filter(|p| p.value > sweep.value_min)
}

/// Points on a lattice `step_km` apart within `radius_km` of `ll`.
fn lattice(ll: [f64; 2], radius_km: f64, step_km: f64) -> Vec<[f64; 2]> {
    let n = (radius_km / step_km).floor() as i64;
    let km_per_deg = EARTH_KM.to_radians();
    let coslat = ll[1].to_radians().cos().max(0.05);
    let mut out = Vec::new();
    for j in -n..=n {
        for i in -n..=n {
            let (dx, dy) = (i as f64 * step_km, j as f64 * step_km);
            if dx.hypot(dy) <= radius_km {
                out.push([ll[0] + dx / (km_per_deg * coslat), ll[1] + dy / km_per_deg]);
            }
        }
    }
    out
}

/// Sweep `s`'s value over the ground point `p`, and the beam's height there (km above the
/// radar). `None` off the sweep or where it has no data; echo under the sweep's threshold reads
/// as its floor.
fn sample_over(s: &BinnedSweep, radar: [f64; 2], p: [f64; 2]) -> Option<(f32, f64)> {
    let (ground_km, az) = crate::xsection::dist_bearing(radar[0], radar[1], p[0], p[1]);
    let (cell, slant) = crate::xsection::gate_over_ground(s, ground_km, az)?;
    let code = s.data[cell];
    if code == 0 {
        return None;
    }
    let v = if code >= 2 {
        s.value_min + (code - 2) as f32 / 253.0 * (s.value_max - s.value_min)
    } else {
        s.value_min
    };
    Some((
        v,
        crate::xsection::beam_height_km(slant, f64::from(s.elevation_deg)),
    ))
}

/// The largest MEHS (mm) within `radius_km` of `ll`, over a 1 km lattice of columns through every
/// reflectivity tilt in `sweeps`, and the searched columns' median. `h0_m` and `hm20_m` are the
/// 0 °C and −20 °C heights above the radar, as [`crate::derived::hail`] takes them.
pub fn hail_peak(
    sweeps: &[BinnedSweep],
    ll: [f64; 2],
    radius_km: f64,
    h0_m: f64,
    hm20_m: f64,
) -> Option<Peak> {
    let s0 = sweeps.first()?;
    let (rlon, rlat) = (f64::from(s0.radar_lon), f64::from(s0.radar_lat));
    let mut buf = Vec::with_capacity(sweeps.len());
    let values: Vec<(f32, [f64; 2])> = lattice(ll, radius_km, 1.0)
        .into_iter()
        .filter_map(|p| {
            let (ground_km, az) = crate::xsection::dist_bearing(rlon, rlat, p[0], p[1]);
            let mehs = crate::derived::mehs_column(sweeps, ground_km, az, h0_m, hm20_m, &mut buf)?;
            Some((mehs, p))
        })
        .collect();
    peak_of(values).filter(|p| p.value > 0.0)
}

/// Heights a BWER's weak echo is looked for at, km above the radar: the mid levels, where the
/// updraft's hole sits under the echo overhang.
const BWER_LEVELS_KM: std::ops::RangeInclusive<f64> = 2.5..=9.0;
/// Radius of the ring of echo that must wall the hole in, km.
const BWER_RING_KM: f64 = 3.0;

/// The strongest BWER candidate within `radius_km` of `ll` (dB: how far its weak echo is under
/// the echo around it), over a 1 km lattice, and the searched points' median (0 where there is no
/// candidate). See [`bwer_at`] for what counts.
pub fn bwer_peak(sweeps: &[BinnedSweep], ll: [f64; 2], radius_km: f64) -> Option<Peak> {
    let s0 = sweeps.first()?;
    let radar = [f64::from(s0.radar_lon), f64::from(s0.radar_lat)];
    // One sweep per elevation: SAILS repeats the low tilts, which never reach the mid levels.
    let mut tilts: Vec<&BinnedSweep> = Vec::new();
    for s in sweeps {
        if !tilts
            .iter()
            .any(|t| (t.elevation_deg - s.elevation_deg).abs() < 0.05)
        {
            tilts.push(s);
        }
    }
    let values: Vec<(f32, [f64; 2])> = lattice(ll, radius_km, 1.0)
        .into_iter()
        .map(|p| (bwer_at(&tilts, radar, p).unwrap_or(0.0), p))
        .collect();
    peak_of(values).filter(|p| p.value > 0.0)
}

/// A bounded weak echo over `p`, in dB, or `None`. On a tilt whose beam is at the mid levels over
/// `p`, the echo there is the hole. It is walled in when at least seven of eight points on a ring
/// 3 km around it, on the same tilt, are strong: 40 dBZ and at least 10 dB over the hole. It
/// counts when it is also capped: a higher tilt over `p`, up to 6 km higher, is as strong (the
/// overhang). Close to the radar the weak echo can rise past the highest tilt; when every higher
/// tilt over `p` is still a walled-in weak echo, the cap is above what the radar sees and is not
/// required. Its value is the ring's median less the hole, the strongest of `p`'s tilts.
fn bwer_at(tilts: &[&BinnedSweep], radar: [f64; 2], p: [f64; 2]) -> Option<f32> {
    let ring: Vec<[f64; 2]> = (0..8)
        .map(|k| {
            let q = crate::rotation::dest(p[0], p[1], f64::from(k) * 45.0, BWER_RING_KM);
            [q.0, q.1]
        })
        .collect();
    /// One tilt over `p`.
    struct Level {
        elevation: f32,
        height_km: f64,
        z: f32,
        /// The ring's median less the echo here, when the ring walls it in.
        walled: Option<f32>,
    }
    let mut levels: Vec<Level> = tilts
        .iter()
        .filter_map(|s| {
            let (z, h) = sample_over(s, radar, p)?;
            // No echo at all is as weak as it gets, but not 32 dB weaker than 0 dBZ.
            let hole = z.max(0.0);
            let strong = (hole + 10.0).max(40.0);
            let walled = (h >= *BWER_LEVELS_KM.start())
                .then(|| {
                    let mut around: Vec<f32> = ring
                        .iter()
                        .filter_map(|q| sample_over(s, radar, *q).map(|v| v.0))
                        .collect();
                    if around.len() < ring.len()
                        || around.iter().filter(|z| **z >= strong).count() < 7
                    {
                        return None;
                    }
                    around.sort_by(f32::total_cmp);
                    Some(around[around.len() / 2] - hole)
                })
                .flatten();
            Some(Level {
                elevation: s.elevation_deg,
                height_km: h,
                z: hole,
                walled,
            })
        })
        .collect();
    levels.sort_by(|a, b| a.elevation.total_cmp(&b.elevation));
    let mut best: Option<f32> = None;
    for (k, l) in levels.iter().enumerate() {
        let Some(contrast) = l.walled else { continue };
        if !BWER_LEVELS_KM.contains(&l.height_km) {
            continue;
        }
        let strong = (l.z + 10.0).max(40.0);
        let above = &levels[k + 1..];
        let capped = above.iter().any(|u| {
            u.height_km > l.height_km && u.height_km <= l.height_km + 6.0 && u.z >= strong
        });
        let cap_unseen = above.iter().all(|u| u.walled.is_some());
        if capped || cap_unseen {
            best = Some(best.map_or(contrast, |b| b.max(contrast)));
        }
    }
    best
}

/// The strongest cyclonic azimuthal shear within `radius_km` of `ll`, and the median of the
/// searched gates' cyclonic shear (gates without an estimate are left out).
pub fn rotation_peak(
    field: &crate::azshear::AzShearField,
    ll: [f64; 2],
    radius_km: f64,
) -> Option<Peak> {
    let radar = [f64::from(field.radar_lon), f64::from(field.radar_lat)];
    let gates = gates_near(
        radar,
        field.az_bins,
        field.gate_count,
        field.first_gate_km - field.gate_interval_km * 0.5,
        field.gate_interval_km,
        ll,
        radius_km,
    );
    let sign = field.cyclonic_sign();
    let values: Vec<(f32, [f64; 2])> = gates
        .into_iter()
        .filter_map(|(az, g, p)| Some((field.at(az, g)?.shear_s * sign, p)))
        .collect();
    peak_of(values)
}

/// Confidence of one fix: the peak's contrast with its surroundings, its strength over the floor,
/// and its nearness to the prediction, each 0..1, multiplied.
fn confidence(peak: &Peak, floor: f32, predicted: [f64; 2], radius_km: f64) -> f32 {
    let contrast = if peak.value > 0.0 {
        ((peak.value - peak.background.max(0.0)) / peak.value).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let strength = ((peak.value - floor) / floor.abs().max(f32::EPSILON) + 0.5).clamp(0.0, 1.0);
    let near =
        (1.0 - 0.5 * dist_km(peak.ll, predicted) / radius_km.max(0.1)).clamp(0.0, 1.0) as f32;
    (contrast * strength * near).clamp(0.0, 1.0)
}

/// Follow a feature through `times` (the loop's volumes, oldest first) from `seed`, the volume
/// and point the analyst picked. `overrides` are fixes the analyst moved by hand, by volume index;
/// each is kept as placed and the track continues from it. `peak_near(volume, point, radius)`
/// finds the strongest value of the feature's kind there. The result has one slot per volume:
/// `None` where the feature was not followed (lost, or past where it was lost).
pub fn track(
    times: &[DateTime<Utc>],
    seed: (usize, [f64; 2]),
    overrides: &[(usize, [f64; 2])],
    params: TrackParams,
    mut peak_near: impl FnMut(usize, [f64; 2], f64) -> Option<Peak>,
) -> Vec<Option<Fix>> {
    let n = times.len();
    let mut out: Vec<Option<Fix>> = vec![None; n];
    let (s, click) = seed;
    if s >= n {
        return out;
    }
    let manual = |i: usize| {
        overrides
            .iter()
            .rev()
            .find(|(j, _)| *j == i)
            .map(|(_, p)| *p)
    };
    // The seed: snapped to the peak right under the click, or the click itself.
    let seed_fix = match manual(s) {
        Some(p) => Fix {
            time: times[s],
            ll: p,
            value: peak_near(s, p, 1.5).map(|k| k.value),
            confidence: 1.0,
            manual: true,
        },
        None => match peak_near(s, click, 3.0).filter(|p| p.value >= params.min_value) {
            Some(p) => Fix {
                time: times[s],
                ll: p.ll,
                value: Some(p.value),
                confidence: confidence(&p, params.min_value, click, 3.0),
                manual: false,
            },
            None => Fix {
                time: times[s],
                ll: click,
                value: None,
                confidence: 0.0,
                manual: true,
            },
        },
    };
    out[s] = Some(seed_fix);
    for forward in [true, false] {
        let order: Vec<usize> = if forward {
            (s + 1..n).collect()
        } else {
            (0..s).rev().collect()
        };
        let mut history: Vec<Fix> = vec![out[s].clone().unwrap()];
        let mut misses = 0;
        for i in order {
            let last = history.last().unwrap();
            let dt = (times[i] - last.time).num_milliseconds() as f64 / 1000.0;
            // Motion from the last two fixes, in degrees per second.
            let (predicted, moving) = match history.len() {
                k if k >= 2 => {
                    let prev = &history[k - 2];
                    let span = (last.time - prev.time).num_milliseconds() as f64 / 1000.0;
                    if span.abs() > 1.0 {
                        let v = [
                            (last.ll[0] - prev.ll[0]) / span,
                            (last.ll[1] - prev.ll[1]) / span,
                        ];
                        ([last.ll[0] + v[0] * dt, last.ll[1] + v[1] * dt], true)
                    } else {
                        (last.ll, false)
                    }
                }
                _ => (last.ll, false),
            };
            let radius = if moving {
                params.search_km
            } else {
                (params.search_km + params.max_speed_ms * dt.abs() / 1000.0).min(25.0)
            };
            if let Some(p) = manual(i) {
                let fix = Fix {
                    time: times[i],
                    ll: p,
                    value: peak_near(i, p, 1.5).map(|k| k.value),
                    confidence: 1.0,
                    manual: true,
                };
                out[i] = Some(fix.clone());
                history.push(fix);
                misses = 0;
                continue;
            }
            match peak_near(i, predicted, radius).filter(|p| p.value >= params.min_value) {
                Some(p) => {
                    let fix = Fix {
                        time: times[i],
                        ll: p.ll,
                        value: Some(p.value),
                        confidence: confidence(&p, params.min_value, predicted, radius),
                        manual: false,
                    };
                    out[i] = Some(fix.clone());
                    history.push(fix);
                    misses = 0;
                }
                None => {
                    misses += 1;
                    if misses >= 2 {
                        break;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn times(n: usize) -> Vec<DateTime<Utc>> {
        let t0 = Utc.with_ymd_and_hms(2013, 5, 20, 19, 40, 0).unwrap();
        (0..n)
            .map(|i| t0 + chrono::Duration::seconds(270 * i as i64))
            .collect()
    }

    /// A synthetic feature moving east-northeast at about 15 m/s, as a sampler that sees it within
    /// the searched radius, plus a weaker distractor that never moves.
    fn moving(i: usize) -> [f64; 2] {
        let km = 15.0 * 270.0 / 1000.0 * i as f64; // ~4 km a volume
        [-97.6 + km / 91.0, 35.30 + km * 0.3 / 111.0]
    }

    fn sampler(lost_at: Option<usize>) -> impl FnMut(usize, [f64; 2], f64) -> Option<Peak> {
        move |i, at, r| {
            let f = moving(i);
            let distractor = [-97.40, 35.20];
            let mut best: Option<Peak> = None;
            if dist_km(f, at) <= r && lost_at.is_none_or(|l| i < l) {
                best = Some(Peak {
                    ll: f,
                    value: 0.012,
                    background: 0.001,
                });
            }
            if dist_km(distractor, at) <= r && best.is_none() {
                best = Some(Peak {
                    ll: distractor,
                    value: 0.005,
                    background: 0.001,
                });
            }
            best
        }
    }

    /// From a click in the middle of the loop, the feature is followed both ways through every
    /// volume, close to where it really was, with high confidence.
    #[test]
    fn a_moving_feature_is_followed_forward_and_back() {
        let t = times(9);
        let click = moving(4);
        let fixes = track(
            &t,
            (4, [click[0] + 0.01, click[1]]),
            &[],
            TrackParams::for_kind(FeatureKind::Rotation),
            sampler(None),
        );
        for (i, f) in fixes.iter().enumerate() {
            let f = f.as_ref().unwrap_or_else(|| panic!("volume {i} lost"));
            assert!(dist_km(f.ll, moving(i)) < 0.5, "volume {i}: {:?}", f.ll);
            assert!(f.confidence > 0.5, "volume {i}: {}", f.confidence);
            assert!(!f.manual);
        }
    }

    /// When the feature dies the track stops after two empty volumes, and does not jump to the
    /// distractor; a hand-placed fix is kept as placed and the track continues from it.
    #[test]
    fn a_lost_feature_ends_the_track_and_a_hand_fix_is_kept() {
        let t = times(9);
        let params = TrackParams::for_kind(FeatureKind::Rotation);
        let fixes = track(&t, (0, moving(0)), &[], params, sampler(Some(6)));
        assert!(fixes[..6].iter().all(Option::is_some));
        assert!(fixes[6..].iter().all(Option::is_none), "{:?}", &fixes[6..]);
        // The analyst moves volume 3's fix 2 km north: it stays there, marked by hand.
        let moved = [moving(3)[0], moving(3)[1] + 0.018];
        let fixes = track(&t, (0, moving(0)), &[(3, moved)], params, sampler(None));
        let f3 = fixes[3].as_ref().unwrap();
        assert!(f3.manual && f3.ll == moved && f3.confidence == 1.0);
        assert!(fixes[4].is_some(), "the track continues from the hand fix");
    }

    const RADAR: [f64; 2] = [-97.28, 35.33];
    /// A VCP 12-like set of tilts, degrees.
    const TILTS: [f32; 14] = [
        0.5, 0.9, 1.3, 1.8, 2.4, 3.1, 4.0, 5.1, 6.4, 8.0, 10.0, 12.5, 15.6, 19.5,
    ];

    /// A synthetic reflectivity volume from `dbz(point, height km)`, filled only where the storm
    /// is (10 to 90 km out, 325 to 35 degrees) and with weak echo elsewhere.
    fn volume(dbz: impl Fn([f64; 2], f64) -> f32) -> Vec<BinnedSweep> {
        let (bins, gates, dg) = (720usize, 200usize, 0.5f32);
        let encode = |z: f32| ((z + 32.0) / 127.0 * 253.0 + 2.0).clamp(2.0, 255.0) as u8;
        TILTS
            .iter()
            .map(|&e| {
                let mut data = vec![encode(5.0); bins * gates];
                for b in 0..bins {
                    let az = (b as f64 + 0.5) * 360.0 / bins as f64;
                    if (35.0..325.0).contains(&az) {
                        continue;
                    }
                    for g in 20..180 {
                        let slant = g as f64 * f64::from(dg);
                        let ground = crate::xsection::ground_from_slant_km(slant, f64::from(e));
                        let q = crate::rotation::dest(RADAR[0], RADAR[1], az, ground);
                        let h = crate::xsection::beam_height_km(slant, f64::from(e));
                        data[b * gates + g] = encode(dbz([q.0, q.1], h));
                    }
                }
                BinnedSweep {
                    moment: crate::level2::Moment::Reflectivity,
                    az_bins: bins,
                    gate_count: gates,
                    data,
                    first_gate_km: 0.0,
                    gate_interval_km: dg,
                    radar_lat: RADAR[1] as f32,
                    radar_lon: RADAR[0] as f32,
                    elevation_deg: e,
                    value_min: -32.0,
                    value_max: 95.0,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// A storm 60 km north of the radar, 58 dBZ at its centre falling 1 dB a km to 10 km out and
    /// 12 km up. With `hole`, a weak echo 2.5 km in radius at 2 to 7 km up, 15 dBZ at its middle,
    /// under the storm's overhang; with `open_east` too, a weak echo 7 km wide runs out the
    /// storm's east side from it (a weak echo region, but not a bounded one).
    fn storm(hole: bool, open_east: bool) -> ([f64; 2], Vec<BinnedSweep>) {
        storm_at(60.0, if hole { 7.0 } else { 0.0 }, open_east)
    }

    /// [`storm`] `range_km` north of the radar with its weak echo (if `hole_top_km` > 0) from 2 km
    /// up to `hole_top_km`.
    fn storm_at(range_km: f64, hole_top_km: f64, open_east: bool) -> ([f64; 2], Vec<BinnedSweep>) {
        let hole = hole_top_km > 0.0;
        let c = crate::rotation::dest(RADAR[0], RADAR[1], 0.0, range_km);
        let c = [c.0, c.1];
        let sweeps = volume(|q, h| {
            let d = dist_km(c, q);
            if d >= 10.0 || h >= 12.0 {
                return 5.0;
            }
            let in_hole = d < 2.5
                || (open_east && {
                    let east = d * (bearing_deg(c, q) - 90.0).to_radians().cos();
                    let across = d * (bearing_deg(c, q) - 90.0).to_radians().sin().abs();
                    east > 0.0 && across < 3.5
                });
            if hole && in_hole && (2.0..hole_top_km).contains(&h) {
                (15.0 + 4.0 * d as f32).min(30.0)
            } else {
                58.0 - d as f32
            }
        });
        (c, sweeps)
    }

    /// A bounded weak echo is found where it is, with its depth against the wall around it; a
    /// plain core has none, and neither does a weak echo open to one side.
    #[test]
    fn a_bounded_weak_echo_is_found_and_an_open_or_absent_one_is_not() {
        let (c, sweeps) = storm(true, false);
        let near = crate::rotation::dest(c[0], c[1], 200.0, 3.0);
        let p = bwer_peak(&sweeps, [near.0, near.1], 6.0).expect("a BWER");
        assert!(dist_km(p.ll, c) < 1.5, "{:?} vs {c:?}", p.ll);
        assert!(p.value >= 25.0, "{}", p.value);
        assert!(p.value >= TrackParams::for_kind(FeatureKind::Bwer).min_value);

        let (c, plain) = storm(false, false);
        assert_eq!(bwer_peak(&plain, c, 6.0), None);
        let (c, open) = storm(true, true);
        assert_eq!(bwer_peak(&open, c, 6.0), None);
        // A weak echo through the storm's whole depth is open at the top, not bounded.
        let (c, chimney) = storm_at(60.0, 12.5, false);
        assert_eq!(bwer_peak(&chimney, c, 6.0), None);
    }

    /// 20 km from the radar the highest tilt passes 7 km up over the storm, inside the weak echo:
    /// its cap is above what the radar sees, and the walled-in weak echo still counts.
    #[test]
    fn a_bounded_weak_echo_rising_past_the_highest_tilt_still_counts() {
        let (c, sweeps) = storm_at(20.0, 9.0, false);
        let p = bwer_peak(&sweeps, c, 4.0).expect("a BWER");
        assert!(dist_km(p.ll, c) < 1.5, "{:?} vs {c:?}", p.ll);
        assert!(p.value >= 25.0, "{}", p.value);
    }

    /// A hail core is found at the storm's core, over the floor, with the freezing levels a
    /// spring day has; weak echo away from the storm grows no hail.
    #[test]
    fn a_hail_core_is_found_at_the_storm_core() {
        let (c, sweeps) = storm(false, false);
        let near = crate::rotation::dest(c[0], c[1], 90.0, 4.0);
        let p = hail_peak(&sweeps, [near.0, near.1], 8.0, 4000.0, 7000.0).expect("a hail core");
        assert!(dist_km(p.ll, c) < 2.5, "{:?} vs {c:?}", p.ll);
        assert!(
            p.value >= TrackParams::for_kind(FeatureKind::Hail).min_value,
            "{}",
            p.value
        );
        assert!(p.background < p.value);
        let away = crate::rotation::dest(RADAR[0], RADAR[1], 180.0, 60.0);
        assert_eq!(
            hail_peak(&sweeps, [away.0, away.1], 8.0, 4000.0, 7000.0),
            None
        );
    }

    /// The gate window finds exactly the gates within the radius of a point on a polar grid.
    #[test]
    fn the_gate_window_holds_the_gates_within_the_radius() {
        let radar = [-97.28, 35.33];
        let ll = crate::rotation::dest(radar[0], radar[1], 250.0, 40.0);
        let ll = [ll.0, ll.1];
        let gates = gates_near(radar, 720, 1832, 2.125, 0.25, ll, 3.0);
        assert!(!gates.is_empty());
        assert!(gates.iter().all(|(_, _, p)| dist_km(*p, ll) <= 3.0));
        // A brute force over a wider window finds no more.
        let wide = gates_near(radar, 720, 1832, 2.125, 0.25, ll, 6.0);
        let inner = wide
            .iter()
            .filter(|(_, _, p)| dist_km(*p, ll) <= 3.0)
            .count();
        assert_eq!(inner, gates.len());
    }

    /// The Moore storm's hail core and BWER, followed through the same KTLX volumes as the
    /// mesocyclone below, with that day's melting level from the Norman sounding. Seeded at
    /// 20:12 near the mesocyclone; both must be found near it and followed through most of the
    /// loop.
    #[tokio::test]
    #[ignore = "network: the KTLX archive and the 2013-05-20 sounding"]
    async fn live_the_moore_hail_core_and_bwer_are_followed_through_the_loop() {
        use crate::level2::{self, Moment};
        let date = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
        let from = Utc.with_ymd_and_hms(2013, 5, 20, 19, 50, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2013, 5, 20, 20, 35, 0).unwrap();
        let cache =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/feature-track");
        std::fs::create_dir_all(&cache).unwrap();
        let at_1212 = Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 0).unwrap();
        let levels = crate::raob::melting_levels(
            &reqwest::Client::new(),
            -97.28,
            35.33,
            at_1212,
            Some(cache.clone()),
        )
        .await
        .unwrap();
        eprintln!(
            "{}: 0 C at {:.0} m, -20 C at {:.0} m",
            levels.label, levels.h0_m, levels.hm20_m
        );
        let ids: Vec<_> = level2::list_volumes("KTLX", date)
            .await
            .unwrap()
            .into_iter()
            .filter(|id| id.date_time().is_some_and(|t| t >= from && t <= to))
            .collect();
        let mut times = Vec::new();
        let mut volumes = Vec::new();
        for id in ids {
            let t = id.date_time().unwrap();
            let scan = level2::download_scan(id, Some(cache.clone()))
                .await
                .unwrap();
            let tilts: Vec<_> = (0..level2::elevation_angles(&scan).len())
                .filter_map(|tilt| {
                    level2::bin_scan_opts_recorded(&scan, Moment::Reflectivity, tilt, false).ok()
                })
                .collect();
            if tilts.len() >= 2 {
                times.push(t);
                volumes.push(tilts);
            }
        }
        let seed = (0..times.len())
            .min_by_key(|&i| (times[i] - at_1212).num_seconds().abs())
            .unwrap();
        let meso = [-97.52, 35.31];
        for kind in [FeatureKind::Hail, FeatureKind::Bwer] {
            let peak = |i: usize, ll: [f64; 2], r: f64| match kind {
                FeatureKind::Hail => hail_peak(&volumes[i], ll, r, levels.h0_m, levels.hm20_m),
                _ => bwer_peak(&volumes[i], ll, r),
            };
            // Where the kind's strongest is within 15 km of the mesocyclone at 20:12.
            let near = peak(seed, meso, 15.0).expect("found near the mesocyclone");
            eprintln!(
                "{}: strongest within 15 km at {:.3}, {:.3}: {:.1} {} ({:.1} km from the mesocyclone)",
                kind.label(),
                near.ll[1],
                near.ll[0],
                near.value,
                kind.unit(),
                dist_km(near.ll, meso)
            );
            let fixes = track(
                &times,
                (seed, near.ll),
                &[],
                TrackParams::for_kind(kind),
                peak,
            );
            let got: Vec<&Fix> = fixes.iter().flatten().collect();
            for f in &got {
                eprintln!(
                    "  {}  {:.3}, {:.3}  {:.1} {}  confidence {:.2}",
                    f.time.format("%H:%M:%S"),
                    f.ll[1],
                    f.ll[0],
                    f.value.unwrap_or(f32::NAN),
                    kind.unit(),
                    f.confidence
                );
            }
            let (first, last) = (got[0], got[got.len() - 1]);
            let hours = (last.time - first.time).num_seconds() as f64 / 3600.0;
            eprintln!(
                "  {} of {} volumes; {:.1} m/s toward {:.0} degrees",
                got.len(),
                times.len(),
                dist_km(first.ll, last.ll) * 1000.0 / (hours * 3600.0).max(1.0),
                bearing_deg(first.ll, last.ll)
            );
            assert!(dist_km(near.ll, meso) < 15.0);
            assert!(
                got.len() * 2 >= times.len(),
                "{} of {}",
                got.len(),
                times.len()
            );
        }
    }

    /// The Moore tornado's mesocyclone, followed through the KTLX volumes of 2013-05-20 from
    /// 19:50 to 20:35 UTC (the archive, cached under `target/tmp/feature-track`), seeded near
    /// Moore at about 20:12. The track must move east-northeast at a storm's speed, as the storm
    /// did, and keep strong shear through its mature phase.
    #[tokio::test]
    #[ignore = "network: the KTLX archive, about 20 volumes"]
    async fn live_the_moore_mesocyclone_is_followed_through_the_loop() {
        use crate::level2::{self, Moment};
        let date = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
        let from = Utc.with_ymd_and_hms(2013, 5, 20, 19, 50, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2013, 5, 20, 20, 35, 0).unwrap();
        let cache =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/feature-track");
        std::fs::create_dir_all(&cache).unwrap();
        let ids: Vec<_> = level2::list_volumes("KTLX", date)
            .await
            .unwrap()
            .into_iter()
            .filter(|id| id.date_time().is_some_and(|t| t >= from && t <= to))
            .collect();
        let mut times = Vec::new();
        let mut fields = Vec::new();
        for id in ids {
            let t = id.date_time().unwrap();
            let scan = level2::download_scan(id, Some(cache.clone()))
                .await
                .unwrap();
            let Ok(vel) = level2::bin_scan_opts_recorded(&scan, Moment::Velocity, 0, true) else {
                continue;
            };
            times.push(t);
            fields.push(crate::azshear::llsd(
                &vel,
                &crate::azshear::LlsdParams::default(),
            ));
        }
        assert!(times.len() >= 8, "{} volumes", times.len());
        let at_1212 = Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 0).unwrap();
        let seed = (0..times.len())
            .min_by_key(|&i| (times[i] - at_1212).num_seconds().abs())
            .unwrap();
        let fixes = track(
            &times,
            (seed, [-97.52, 35.31]),
            &[],
            TrackParams::for_kind(FeatureKind::Rotation),
            |i, ll, r| rotation_peak(&fields[i], ll, r),
        );
        let got: Vec<&Fix> = fixes.iter().flatten().collect();
        for f in &got {
            eprintln!(
                "{}  {:.3}, {:.3}  shear {:.4}  confidence {:.2}",
                f.time.format("%H:%M:%S"),
                f.ll[1],
                f.ll[0],
                f.value.unwrap_or(f32::NAN),
                f.confidence
            );
        }
        assert!(got.len() >= 6, "followed through {} volumes", got.len());
        let (first, last) = (got[0], got[got.len() - 1]);
        let hours = (last.time - first.time).num_seconds() as f64 / 3600.0;
        let speed_ms = dist_km(first.ll, last.ll) * 1000.0 / (hours * 3600.0);
        let heading = bearing_deg(first.ll, last.ll);
        eprintln!("speed {speed_ms:.1} m/s toward {heading:.0} degrees over {hours:.2} h");
        assert!((5.0..25.0).contains(&speed_ms), "{speed_ms}");
        assert!((45.0..120.0).contains(&heading), "{heading}");
        assert!(
            got.iter()
                .filter(|f| f.value.is_some_and(|v| v >= 0.01))
                .count()
                >= 3
        );
    }
}
