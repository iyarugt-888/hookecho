//! Following an analyst-picked feature through a radar loop (ROADMAP_NEW R4, 1010.md V14).
//!
//! The analyst points at a mesocyclone or a reflectivity core in one volume. [`track`] follows it
//! through the loop's other volumes, forward and back: from each fix it predicts the next one
//! from the feature's own motion so far, searches around the prediction for the strongest value
//! of the feature's kind ([`rotation_peak`]: cyclonic LLSD azimuthal shear; [`core_peak`]:
//! reflectivity), and stops after two volumes in a row where nothing strong enough is there.
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
}

impl FeatureKind {
    pub fn label(self) -> &'static str {
        match self {
            FeatureKind::Rotation => "rotation",
            FeatureKind::Core => "core",
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
