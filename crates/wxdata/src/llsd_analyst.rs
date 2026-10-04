//! The new rotation pipeline as an analyst sees it (detectionplan.md Phases 12 and 13).
//!
//! [`analyse`] takes one volume's tracked LLSD rotation columns ([`crate::rotation_tracks`]) and its
//! debris signatures, classifies the debris ([`crate::debris_class`]), fuses each column
//! ([`crate::tornado_fusion`]) and returns them strongest first. [`Analysed::lines`] says why each
//! one exists: every member tilt's measurements and quality, the track, the debris beside it, and
//! every term of its evidence score. A false alarm can then be diagnosed rather than only seen.
//!
//! [`identify`] and [`circulations`] turn analysed columns into Tornado ID's own
//! [`crate::tornado_id::TornadoId`] and [`crate::tornado_id::Circulation`], so the app can show
//! the fusion as its Tornado ID (detectionplan.md Phase 13: held out by event on 25 events, at
//! about 1.5 false alarms per radar-hour it found more tornadoes than the legacy Tornado ID with a
//! far lower false-alarm ratio). The legacy rotation and debris layers, and their alerts, are not
//! touched.

use crate::confirm::Confirmation;
use crate::debris_class::{
    association_radius_km, classify, DebrisAssessment, DebrisClass, DebrisParams, HailSign,
};
use crate::rotation::CoupletHit;
use crate::rotation::Sense;
use crate::rotation_tracks::Tracked;
use crate::tds::TdsHit;
use crate::tornado_fusion::{fuse, Features, Fused, WEIGHTS};
use crate::tornado_id::{Circulation, Evidence, Member, Tier, TornadoId, MERGE_KM};

/// Fused evidence at and above which a circulation is a Tornado ID detection at all (the
/// Possible tier). Held out by event on the 25-event backtest, everything at and above 0.3 found
/// more tornadoes than everything the legacy Tornado ID shows, with fewer false alarms (POD 0.54,
/// FAR 0.53, 7.7 per radar-hour, against 0.49, 0.77 and 10.1). It is also where a strong,
/// deep circulation with no debris yet (40-50 m/s, three tilts) begins to show: at 0.4 it did
/// not, where the legacy Tornado ID read it as Likely.
pub const MIN_SCORE: f32 = 0.3;
/// Fused evidence at and above which it reads as Likely, or as Debris with a tornado debris
/// signature beside it: the strict operating point (about 1.5 false alarms per radar-hour).
pub const LIKELY_SCORE: f32 = 0.6;
/// The rotation-only Possible bar applies from this range (km) out. Nearer, the lowest beam is
/// under ~0.6 km, a tornado's debris is usually in view and the fused score already finds it,
/// and the beam resolves ordinary storm shear sharply enough to reach the bar: lifted markers
/// within 40 km verified 0 of 13 times on 139 random severe-weather windows and 1 of 19 on the
/// 65-event corpus, against 60% for the fused markers there. Lifting only from 40 km lost no
/// tornado on either set (detectionplan.md).
pub const LIFT_MIN_RANGE_KM: f32 = 40.0;
/// A Possible verdict this near (km) a wind turbine in service is wind-farm clutter and is not
/// shown ([`VerdictOptions::turbines_in_year`]). Every marker within 2 km of one was false: 25 on
/// 139 random severe-weather windows, 22 on the 65-event corpus, all at Possible. Masking them cut
/// the random sample's false Possible markers from 0.85 to 0.60 per radar-hour and lost no
/// tornado on either set; at 3 km the corpus lost one (detectionplan.md).
pub const WIND_TURBINE_KM: f64 = 2.0;

/// How verdicts are drawn from analysed columns, beyond their evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VerdictOptions {
    /// The rotation-only Possible bar (s⁻¹) of [`Analysed::tornado_id_with`]; `None` is off.
    pub rotation_only_possible: Option<f32>,
    /// The volume's year. A Possible verdict within [`WIND_TURBINE_KM`] of a wind turbine in
    /// service by then is not shown ([`crate::wind_turbines`]); Likely and above, and anything a
    /// tornado report confirms, still are, so a tornado through a wind farm is not hidden once
    /// its evidence is strong. `None` skips the mask.
    pub turbines_in_year: Option<i32>,
}

impl VerdictOptions {
    /// The rotation-only bar alone, with no wind-turbine mask.
    pub fn bar(rotation_only_possible: Option<f32>) -> Self {
        Self {
            rotation_only_possible,
            turbines_in_year: None,
        }
    }
}

/// One circulation, analysed.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysed {
    pub tracked: Tracked,
    pub features: Features,
    pub fused: Fused,
    /// The debris signatures within the association radius of the column, classified, nearest
    /// first, with their distance (km).
    pub debris: Vec<(TdsHit, DebrisAssessment, f32)>,
}

/// Analyse one volume: `tracked` from [`crate::rotation_tracks::Tracker::update`], `debris` the
/// volume's debris signatures (raw, uncorroborated) and `hail_cores` its hail cores as
/// `(lon, lat, posh 0..1)`, empty when there are none. Strongest evidence first.
pub fn analyse(
    tracked: Vec<Tracked>,
    debris: &[TdsHit],
    hail_cores: &[(f64, f64, f32)],
) -> Vec<Analysed> {
    let columns: Vec<_> = tracked.iter().map(|t| t.column.clone()).collect();
    let p = DebrisParams::default();
    let assessed: Vec<(TdsHit, DebrisAssessment)> = debris
        .iter()
        .map(|h| (*h, classify(h, &columns, hail_cores, &p)))
        .collect();
    let mut out: Vec<Analysed> = tracked
        .into_iter()
        .map(|t| {
            let features = Features::of(&t, &assessed);
            let fused = fuse(&features, &WEIGHTS);
            let c = &t.column;
            let mut near: Vec<(TdsHit, DebrisAssessment, f32)> = assessed
                .iter()
                .map(|(h, a)| {
                    let d = crate::tds::ground_km((h.lon, h.lat), (c.lon, c.lat)) as f32;
                    (*h, a.clone(), d)
                })
                .filter(|(h, _, d)| *d <= association_radius_km(h.range_km, &p))
                .collect();
            near.sort_by(|a, b| a.2.total_cmp(&b.2));
            Analysed {
                tracked: t,
                features,
                fused,
                debris: near,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.fused
            .score
            .total_cmp(&a.fused.score)
            .then(a.tracked.track_id.cmp(&b.tracked.track_id))
    });
    out
}

/// Tornado ID verdicts from analysed columns, strongest tier first. `confirm` says what people
/// said about a place (reports, observed warnings).
pub fn identify(
    analysed: &[Analysed],
    confirm: impl Fn(f64, f64) -> Confirmation,
) -> Vec<TornadoId> {
    identify_with(analysed, confirm, VerdictOptions::default())
}

/// [`identify`], with the rotation-only bar of [`Analysed::tornado_id_with`] and the wind-turbine
/// mask ([`VerdictOptions`]).
pub fn identify_with(
    analysed: &[Analysed],
    confirm: impl Fn(f64, f64) -> Confirmation,
    options: VerdictOptions,
) -> Vec<TornadoId> {
    let mut out: Vec<TornadoId> = analysed
        .iter()
        .filter_map(|a| {
            let c = &a.tracked.column;
            let id = a.tornado_id_with(&confirm(c.lon, c.lat), options.rotation_only_possible)?;
            let clutter = id.tier == Tier::Possible
                && options.turbines_in_year.is_some_and(|year| {
                    crate::wind_turbines::near(c.lon, c.lat, year, WIND_TURBINE_KM)
                });
            (!clutter).then_some(id)
        })
        .collect();
    out.sort_by(|a, b| b.tier.cmp(&a.tier).then(b.score.total_cmp(&a.score)));
    out
}

/// [`identify`], as one detection per tornado with the legacy couplets and debris signatures
/// within [`MERGE_KM`] tied to it (indices into `couplets` and `debris`, as the app draws them),
/// so merged mode still draws each tornado once. Each legacy detection is tied to at most one, the
/// strongest first.
pub fn circulations(
    analysed: &[Analysed],
    couplets: &[CoupletHit],
    debris: &[TdsHit],
    confirm: impl Fn(f64, f64) -> Confirmation,
) -> Vec<Circulation> {
    circulations_with(
        analysed,
        couplets,
        debris,
        confirm,
        VerdictOptions::default(),
    )
}

/// [`circulations`], with the [`VerdictOptions`] of [`identify_with`].
pub fn circulations_with(
    analysed: &[Analysed],
    couplets: &[CoupletHit],
    debris: &[TdsHit],
    confirm: impl Fn(f64, f64) -> Confirmation,
    options: VerdictOptions,
) -> Vec<Circulation> {
    // One marker per tornado. A tornado's low-level circulation is often several rotation columns
    // a few km apart (Mayfield 2021: 2-5 verdicts within 6 km on every scan), and each was drawn
    // as its own marker. The strongest verdict (highest tier, then score; `identify_with` sorts
    // them so) leads; any other within MERGE_KM is the same tornado and is folded into it, the
    // same radius that ties the original detectors' couplets and debris to it below.
    let mut ids: Vec<TornadoId> = Vec::new();
    let mut folded: Vec<usize> = Vec::new();
    for id in identify_with(analysed, confirm, options) {
        match ids
            .iter()
            .position(|k| crate::tds::ground_km((k.lon, k.lat), (id.lon, id.lat)) <= MERGE_KM)
        {
            Some(i) => folded[i] += 1,
            None => {
                ids.push(id);
                folded.push(0);
            }
        }
    }
    for (id, n) in ids.iter_mut().zip(&folded) {
        if *n > 0 {
            id.reasons.insert(
                0,
                format!(
                    "{n} more rotation column{} within {MERGE_KM:.0} km read as this same tornado",
                    if *n == 1 { "" } else { "s" }
                ),
            );
        }
    }
    let (mut tied_c, mut tied_d) = (vec![false; couplets.len()], vec![false; debris.len()]);
    ids.into_iter()
        .map(|id| {
            let here = (id.lon, id.lat);
            let mut members: Vec<Member> = Vec::new();
            for (i, c) in couplets.iter().enumerate() {
                let km = crate::tds::ground_km(here, (c.lon, c.lat));
                if !tied_c[i] && km <= MERGE_KM {
                    tied_c[i] = true;
                    members.push(Member {
                        evidence: Evidence::Rotation(i),
                        lon: c.lon,
                        lat: c.lat,
                        confidence: c.confidence,
                        km,
                    });
                }
            }
            for (i, d) in debris.iter().enumerate() {
                let km = crate::tds::ground_km(here, (d.lon, d.lat));
                if !tied_d[i] && km <= MERGE_KM {
                    tied_d[i] = true;
                    members.push(Member {
                        evidence: Evidence::Debris(i),
                        lon: d.lon,
                        lat: d.lat,
                        confidence: d.confidence,
                        km,
                    });
                }
            }
            members.sort_by(|a, b| a.km.total_cmp(&b.km));
            Circulation { id, members }
        })
        .collect()
}

/// Compass bearing (degrees from north) of motion `(east, north)`.
fn bearing(u: f32, v: f32) -> f32 {
    u.atan2(v).to_degrees().rem_euclid(360.0)
}

fn opt(v: Option<f32>, fmt: impl Fn(f32) -> String) -> String {
    v.map_or("none".into(), fmt)
}

impl Analysed {
    /// This circulation as a Tornado ID verdict, with what people said about it. `None` when its
    /// evidence is under [`MIN_SCORE`], or it sits in no convective core, and no tornado report
    /// confirms it: an observed warning raises the tier of a detection but cannot make one (its
    /// polygon is a county wide).
    pub fn tornado_id(&self, confirmation: &Confirmation) -> Option<TornadoId> {
        self.tornado_id_with(confirmation, None)
    }

    /// [`Self::tornado_id`], with an opt-in bar (s⁻¹): a rooted, cyclonic column in a convective
    /// core, at least [`LIFT_MIN_RANGE_KM`] out, with no tornado debris signature beside it,
    /// reaches Possible once its 0-2 km shear is at least the bar,
    /// whatever its evidence score. Its score is unchanged and its first reason says why it is
    /// shown.
    ///
    /// `None` here is off; the app starts at 0.018. Rotation-only tornadoes with strong low-level shear (0.017-0.028 s⁻¹ on the
    /// random 2022-2024 sample) fuse at 0.22-0.25, under Possible, because debris carries most of
    /// the weight. Lifting them at 0.018 s⁻¹ found 59 more tornado reports and paths on the
    /// 65-event corpus for 221 more false detections, and on the random sample brought the fused
    /// Possible tier to the original Tornado ID's false-alarm rate. Lifted markers verify about a
    /// fifth of the time, against about two-fifths for the rest of Possible (detectionplan.md).
    pub fn tornado_id_with(
        &self,
        confirmation: &Confirmation,
        rotation_only_possible: Option<f32>,
    ) -> Option<TornadoId> {
        let score = self.fused.score;
        let c = &self.tracked.column;
        let tds = self
            .debris
            .iter()
            .filter(|(_, a, _)| a.class == DebrisClass::TornadoDebrisSignature)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let lifted = score < MIN_SCORE
            && confirmation.report.is_none()
            && rotation_only_possible.is_some_and(|bar| {
                c.rooted
                    && c.sense == Sense::Cyclonic
                    && tds.is_none()
                    && c.low_level_azshear.is_some_and(|s| s >= bar)
                    // In a convective core: a >= 40 dBZ object within 5 km (`storm_mode`).
                    // Stratiform rain and strong synoptic wind make rotation-like shear with no
                    // core; requiring one kept every tornado the bar gains while halving its
                    // false markers on ordinary severe days (146 to 71 over 95 radar-hours).
                    && c.echo.is_some()
                    && c.members.first().is_some_and(|m| m.object.range_km >= LIFT_MIN_RANGE_KM)
            });
        // Every verdict sits in a convective core (`echo`: a >= 40 dBZ object within 5 km) unless a
        // tornado report confirms it. Shear with no core is stratiform rain, synoptic wind or
        // clutter: on 139 random severe-weather windows the rule removed 61 of 134 false markers at
        // Possible and all 5 at Likely, on the 65-event corpus 23 of 172 and 6 of 35, and it lost
        // no tornado on any set (detectionplan.md).
        let coreless = c.echo.is_none();
        if confirmation.report.is_none() && (coreless || (score < MIN_SCORE && !lifted)) {
            return None;
        }
        let tier = if confirmation.level().is_some() {
            Tier::Confirmed
        } else if score >= LIKELY_SCORE && tds.is_some() {
            Tier::Debris
        } else if score >= LIKELY_SCORE {
            Tier::Likely
        } else {
            Tier::Possible
        };
        let mut reasons = Vec::new();
        if lifted {
            reasons.push(format!(
                "Shown for strong low-level rotation without debris ({:.3} s\u{207b}\u{b9}, \
                 at or above the {:.3} s\u{207b}\u{b9} bar); its evidence score alone is \
                 under Possible",
                c.low_level_azshear.unwrap_or(0.0),
                rotation_only_possible.unwrap_or(0.0)
            ));
        }
        if let Some(line) = confirmation.describe() {
            reasons.push(line);
        }
        reasons.extend(self.lines());
        Some(TornadoId {
            lon: c.lon,
            lat: c.lat,
            tier,
            score,
            // The fused score is one logistic term; its log-odds parts are in the reasons.
            terms: vec![crate::tornado_id::Term {
                label: "fused evidence",
                value: score,
            }],
            vrot_ms: c
                .members
                .iter()
                .map(|m| m.object.max_delta_v_ms / 2.0)
                .reduce(f32::max),
            min_cc: tds.map(|(h, _, _)| h.min_cc),
            reasons,
        })
    }

    /// One line for a marker label.
    pub fn headline(&self) -> String {
        let c = &self.tracked.column;
        format!(
            "LLSD {:.3} s⁻¹ · {}t {:.1}-{:.1} km · evidence {}",
            c.max_azshear,
            c.tilts(),
            c.base_km,
            c.top_km,
            crate::evidence::out_of_100(self.fused.score)
        )
    }

    /// Why it exists, line by line: the column, each member tilt, the track, the debris beside
    /// it, and every term of the evidence score (largest first; terms that are zero are left out).
    pub fn lines(&self) -> Vec<String> {
        let t = &self.tracked;
        let c = &t.column;
        let mut out = Vec::new();
        let sense = match c.sense {
            Sense::Cyclonic => "cyclonic",
            Sense::Anticyclonic => "anticyclonic",
        };
        let rooted = if !c.rooted {
            "not rooted in the lowest tilt".to_string()
        } else if self.features.weak_echo_root > 0.0 {
            "rooted in the lowest tilt, through weak echo".to_string()
        } else {
            "rooted in the lowest tilt".to_string()
        };
        out.push(format!(
            "Column: {sense}, {} tilt{}, {:.1}-{:.1} km above the radar, {rooted}",
            c.tilts(),
            if c.tilts() == 1 { "" } else { "s" },
            c.base_km,
            c.top_km
        ));
        if let (Some(lean), Some(b)) = (c.lean_km_per_km, c.lean_bearing_deg) {
            out.push(format!(
                "Leans {lean:.1} km per km of height toward {b:.0}°"
            ));
        }
        out.push(format!(
            "Peak shear by layer: 0-2 km {}, 2-3 km {}, 3-6 km {}",
            opt(c.low_level_azshear, |s| format!("{s:.3} s⁻¹")),
            opt(c.transition_azshear, |s| format!("{s:.3} s⁻¹")),
            opt(c.mid_level_azshear, |s| format!("{s:.3} s⁻¹")),
        ));
        for m in &c.members {
            let o = &m.object;
            out.push(format!(
                "  {:.1}° at {:.1} km: max {:.3} / p90 {:.3} / median {:.3} s⁻¹, ΔV {:.0} m/s \
                 (robust {:.0}), {:.1} km², {:.1}×{:.1} km, texture {:.1} m/s, fit RMSE {:.1} m/s, \
                 significance {:.0}σ, fold pairs {:.2}/row{}",
                o.elevation_deg,
                o.beam_height_km,
                o.max_azshear,
                o.p90_azshear,
                o.median_azshear,
                o.max_delta_v_ms,
                o.robust_delta_v_ms,
                o.area_km2,
                o.length_km,
                o.width_km,
                o.mean_texture_ms,
                o.fit_rmse_ms,
                o.significance,
                o.fold_crossings,
                if m.weak_echo { ", in weak echo" } else { "" }
            ));
        }
        let motion = t.motion_ms.map_or("no motion yet".to_string(), |(u, v)| {
            format!("moving {:.0} m/s toward {:.0}°", u.hypot(v), bearing(u, v))
        });
        out.push(format!(
            "Track #{}: {} volume{}, {} min, {motion}, shear trend {}",
            t.track_id,
            t.age_volumes,
            if t.age_volumes == 1 { "" } else { "s" },
            t.age_seconds / 60,
            opt(t.azshear_trend, |s| format!("{s:+.3} s⁻¹ per 10 min")),
        ));
        if self.debris.is_empty() {
            out.push("No debris signature beside it".into());
        }
        for (h, a, km) in &self.debris {
            let hail: Vec<String> = a
                .hail_signs
                .iter()
                .map(|s| match s {
                    HailSign::NegativeZdr(z) => format!("ZDR {z:.1} dB"),
                    HailSign::HailCore { posh, km } => {
                        format!("hail core {:.0}% POSH {km:.1} km away", posh * 100.0)
                    }
                })
                .collect();
            out.push(format!(
                "Debris {km:.1} km away: {}, polarimetric evidence {}, CC down to {:.2} in {:.0} \
                 dBZ{}{}",
                a.class.label(),
                crate::evidence::out_of_100(a.polarimetric),
                h.min_cc,
                h.max_z,
                h.zdr_db
                    .map_or(String::new(), |z| format!(", ZDR {z:.1} dB")),
                if hail.is_empty() {
                    String::new()
                } else {
                    format!(", hail signs: {}", hail.join(", "))
                }
            ));
        }
        out.push(format!(
            "Evidence {} (not a probability), in log-odds:",
            crate::evidence::out_of_100(self.fused.score)
        ));
        let mut terms: Vec<_> = self.fused.terms.iter().filter(|t| t.logit != 0.0).collect();
        terms.sort_by(|a, b| b.logit.abs().total_cmp(&a.logit.abs()));
        for term in terms {
            out.push(format!(
                "  {:+.2}  {}",
                term.logit,
                term.label.replace('_', " ")
            ));
        }
        out.push(
            "Experimental: raises no alert and feeds nothing else until it is validated".into(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confirm::Confirmation;
    use crate::rotation_columns::{ColumnMember, RotationColumn};
    use crate::rotation_objects::RotationObject;

    fn east(km: f64) -> f64 {
        -97.0 + km / (111.32 * 35f64.to_radians().cos())
    }

    fn member(tilt: usize, h: f32, shear: f32, weak_echo: bool) -> ColumnMember {
        ColumnMember {
            tilt,
            weak_echo,
            object: RotationObject {
                lon: east(0.0),
                lat: 35.0,
                area_km2: 6.0,
                diameter_km: 2.8,
                length_km: 3.0,
                width_km: 2.0,
                max_azshear: shear,
                p90_azshear: shear * 0.9,
                median_azshear: shear * 0.6,
                robust_delta_v_ms: 40.0,
                max_delta_v_ms: 50.0,
                mean_texture_ms: 3.0,
                fit_rmse_ms: 3.0,
                valid_fraction: 1.0,
                fold_share: 0.0,
                fold_crossings: 0.0,
                significance: 25.0,
                range_km: 40.0,
                beam_height_km: h,
                elevation_deg: 0.5 + tilt as f32 * 0.4,
                sense: Sense::Cyclonic,
                gates: 60,
                radials: 6,
                artifacts: Vec::new(),
            },
        }
    }

    fn tracked(age: usize) -> Tracked {
        let members = vec![member(0, 0.4, 0.03, true), member(1, 0.8, 0.025, false)];
        Tracked {
            column: RotationColumn {
                lon: east(0.0),
                lat: 35.0,
                sense: Sense::Cyclonic,
                base_km: 0.4,
                top_km: 0.8,
                depth_km: 0.4,
                rooted: true,
                low_level_azshear: Some(0.03),
                transition_azshear: None,
                mid_level_azshear: None,
                max_azshear: 0.03,
                integrated_azshear: 0.011,
                lean_km_per_km: None,
                lean_bearing_deg: None,
                echo: Some(crate::storm_mode::EchoShape {
                    length_km: 20.0,
                    width_km: 12.0,
                    area_km2: 180.0,
                }),
                near_flow: None,
                members,
            },
            track_id: 7,
            age_volumes: age,
            age_seconds: (age as i64 - 1) * 300,
            motion_ms: (age > 1).then_some((10.0, 10.0)),
            position_jump_km: None,
            azshear_trend: (age > 1).then_some(0.002),
            low_level_trend: None,
            depth_trend: None,
        }
    }

    fn debris_ball(km: f64, zdr: f32) -> TdsHit {
        TdsHit {
            lon: east(km),
            lat: 35.0,
            gates: 20,
            min_cc: 0.25,
            mean_cc: 0.6,
            mean_z: 52.0,
            max_z: 60.0,
            area_km2: 3.0,
            contrast: Some(0.2),
            range_km: 40.0,
            tilts: 2,
            top_km: 1.0,
            base_km: 0.4,
            rooted: Some(true),
            rotation_ms: None,
            unrotated: false,
            zdr_db: Some(zdr),
            confirmation: Confirmation::default(),
            raw_confidence: 0.75,
            confidence: 0.75,
        }
    }

    #[test]
    fn rotation_columns_a_few_km_apart_are_one_tornado_marker() {
        // Two strong columns of one circulation, 3 km apart: one marker, the stronger leading.
        let near = |km: f64, id: u64| {
            let mut t = tracked(3);
            t.track_id = id;
            t.column.lon = east(km);
            for m in &mut t.column.members {
                m.object.lon = east(km);
            }
            t
        };
        let none = Confirmation::default();
        let two = analyse(
            vec![near(0.0, 1), near(3.0, 2)],
            &[debris_ball(0.5, 0.5)],
            &[],
        );
        assert!(
            two.iter().all(|a| a.fused.score >= MIN_SCORE),
            "both would show"
        );
        let circs = circulations(&two, &[], &[], |_, _| none);
        assert_eq!(circs.len(), 1, "one tornado, one marker");
        assert!(circs[0].id.reasons[0].starts_with("1 more rotation column within 15 km"));
        // Thirty km apart they are two tornadoes.
        let apart = analyse(vec![near(0.0, 1), near(30.0, 2)], &[], &[]);
        assert_eq!(circulations(&apart, &[], &[], |_, _| none).len(), 2);
    }

    #[test]
    fn strong_rotation_without_debris_reaches_possible_only_when_opted_in() {
        // One volume, no debris: strong low-level shear (0.03 s^-1) that fuses under Possible.
        let mut t = tracked(1);
        t.column.low_level_azshear = Some(0.021);
        t.column.max_azshear = 0.021;
        t.column.echo = Some(crate::storm_mode::EchoShape {
            length_km: 20.0,
            width_km: 12.0,
            area_km2: 180.0,
        });
        for m in &mut t.column.members {
            m.object.max_azshear = 0.021;
        }
        let a = &analyse(vec![t.clone()], &[], &[])[0];
        assert!(a.fused.score < MIN_SCORE, "{}", a.fused.score);
        let none = Confirmation::default();
        assert!(a.tornado_id(&none).is_none(), "off by default");
        assert!(
            a.tornado_id_with(&none, Some(0.022)).is_none(),
            "under the bar"
        );
        let id = a.tornado_id_with(&none, Some(0.020)).expect("lifted");
        assert_eq!(id.tier, Tier::Possible);
        assert_eq!(id.score, a.fused.score, "the evidence score is not changed");
        assert!(id.reasons[0].starts_with("Shown for strong low-level rotation without debris"));
        // Not for anticyclonic rotation, an unrooted column, or one with debris beside it.
        let mut anti = t.clone();
        anti.column.sense = Sense::Anticyclonic;
        let anti = &analyse(vec![anti], &[], &[])[0];
        assert!(anti.tornado_id_with(&none, Some(0.020)).is_none());
        // No convective core near it (stratiform rain, synoptic wind): not lifted.
        let mut coreless = t.clone();
        coreless.column.echo = None;
        let coreless = &analyse(vec![coreless], &[], &[])[0];
        assert!(coreless.tornado_id_with(&none, Some(0.020)).is_none());
        // Within 40 km, where debris would be in view: not lifted.
        let mut close = t.clone();
        for m in &mut close.column.members {
            m.object.range_km = 30.0;
        }
        let close = &analyse(vec![close], &[], &[])[0];
        assert!(close.fused.score < MIN_SCORE, "{}", close.fused.score);
        assert!(close.tornado_id_with(&none, Some(0.020)).is_none());
        let mut aloft = t.clone();
        aloft.column.rooted = false;
        let aloft = &analyse(vec![aloft], &[], &[])[0];
        assert!(aloft.tornado_id_with(&none, Some(0.020)).is_none());
        // identify_with passes the bar through.
        assert_eq!(
            identify_with(
                &analyse(vec![t], &[], &[]),
                |_, _| none,
                VerdictOptions::bar(Some(0.020))
            )
            .len(),
            1
        );
    }

    #[test]
    fn a_possible_verdict_beside_a_wind_turbine_is_not_shown() {
        // A Possible-tier column standing on the 25 Mile Creek wind farm (online 2022).
        let place = |t: &mut Tracked, lon: f64, lat: f64| {
            t.column.lon = lon;
            t.column.lat = lat;
            for m in &mut t.column.members {
                m.object.lon = lon;
                m.object.lat = lat;
            }
        };
        let mut t = tracked(1);
        t.column.low_level_azshear = Some(0.021);
        t.column.max_azshear = 0.021;
        for m in &mut t.column.members {
            m.object.max_azshear = 0.021;
        }
        place(&mut t, -99.7786, 36.5025);
        let none = Confirmation::default();
        let a = analyse(vec![t.clone()], &[], &[]);
        let on = |year| VerdictOptions {
            rotation_only_possible: Some(0.020),
            turbines_in_year: Some(year),
        };
        assert_eq!(
            identify_with(&a, |_, _| none, on(2021))[0].tier,
            Tier::Possible
        );
        assert!(
            identify_with(&a, |_, _| none, on(2023)).is_empty(),
            "clutter"
        );
        assert_eq!(
            identify_with(&a, |_, _| none, VerdictOptions::bar(Some(0.020))).len(),
            1,
            "no mask without a year"
        );
        // A tornado report there still shows it, and Likely evidence is not masked.
        let report = |_: f64, _: f64| Confirmation {
            observed_warning: false,
            report: Some((2.0, 3)),
        };
        assert_eq!(identify_with(&a, report, on(2023))[0].tier, Tier::Confirmed);
        let mut strong = tracked(3);
        place(&mut strong, -99.7786, 36.5025);
        let mut ball = debris_ball(0.0, 0.5);
        (ball.lon, ball.lat) = (-99.7786, 36.5025);
        let strong = analyse(vec![strong], &[ball], &[]);
        let ids = identify_with(&strong, |_, _| none, on(2023));
        assert!(
            ids.first().is_some_and(|id| id.tier >= Tier::Likely),
            "{ids:?}"
        );
    }

    #[test]
    fn debris_beside_a_column_raises_its_evidence_and_is_explained() {
        let with = analyse(vec![tracked(3)], &[debris_ball(1.0, 0.5)], &[]);
        let without = analyse(vec![tracked(3)], &[], &[]);
        assert!(with[0].fused.score > without[0].fused.score);
        assert_eq!(with[0].debris.len(), 1);
        let lines = with[0].lines();
        let text = lines.join("\n");
        assert!(
            text.contains("rooted in the lowest tilt, through weak echo"),
            "{text}"
        );
        assert!(text.contains("tornado debris signature"), "{text}");
        assert!(
            text.contains("Track #7: 3 volumes, 10 min, moving 14 m/s toward 45°"),
            "{text}"
        );
        assert!(text.contains("in weak echo"), "{text}");
        assert!(
            !text.contains('%') || text.contains("POSH"),
            "no percentages: {text}"
        );
        // The listed terms are the score's own.
        let listed: f32 = lines
            .iter()
            .filter_map(|l| l.split_whitespace().next()?.parse::<f32>().ok())
            .sum();
        let z: f32 = with[0].fused.terms.iter().map(|t| t.logit).sum();
        assert!((listed - z).abs() < 0.02, "{listed} vs {z}");
    }

    #[test]
    fn hail_signs_are_named() {
        let a = analyse(vec![tracked(1)], &[debris_ball(1.0, -1.5)], &[]);
        let text = a[0].lines().join("\n");
        assert!(text.contains("hail signs: ZDR -1.5 dB"), "{text}");
        assert!(text.contains("no motion yet"), "{text}");
    }

    #[test]
    fn a_track_that_does_not_move_is_stationary() {
        // Seen in 3 volumes and moving 0.7 m/s: fixed clutter, a wind farm.
        let mut still = tracked(3);
        still.motion_ms = Some((0.5, 0.5));
        assert_eq!(analyse(vec![still], &[], &[])[0].features.stationary, 1.0);
        // Moving 14 m/s, or seen only twice, it is not.
        assert_eq!(
            analyse(vec![tracked(3)], &[], &[])[0].features.stationary,
            0.0
        );
        let mut young = tracked(2);
        young.motion_ms = Some((0.5, 0.5));
        assert_eq!(analyse(vec![young], &[], &[])[0].features.stationary, 0.0);
    }

    fn couplet(lon: f64) -> CoupletHit {
        CoupletHit {
            lon,
            lat: 35.0,
            vrot_ms: 25.0,
            g2g_ms: 50.0,
            range_km: 40.0,
            gates: 20,
            tilts: 2,
            top_km: 1.0,
            base_km: 0.4,
            rooted: Some(true),
            sense: Sense::Cyclonic,
            debris_confidence: None,
            confidence: 0.6,
            raw_confidence: 0.6,
            confirmation: Confirmation::default(),
        }
    }

    #[test]
    fn a_fused_circulation_reads_as_a_tornado_id_tier() {
        let none = |_: f64, _: f64| Confirmation::default();
        // Strong rotation and debris beside it: the Debris tier, with the debris ball's CC.
        let with = analyse(vec![tracked(3)], &[debris_ball(1.0, 0.5)], &[]);
        assert!(
            with[0].fused.score >= LIKELY_SCORE,
            "{}",
            with[0].fused.score
        );
        let id = &identify(&with, none)[0];
        assert_eq!(id.tier, Tier::Debris);
        assert_eq!(id.min_cc, Some(0.25));
        assert_eq!(id.score, with[0].fused.score);
        assert!(id
            .reasons
            .iter()
            .any(|r| r.contains("tornado debris signature")));
        // Weak rotation alone is below the floor: not a detection at all.
        let mut weak = tracked(1);
        for m in &mut weak.column.members {
            m.object.max_azshear = 0.008;
        }
        weak.column.max_azshear = 0.008;
        weak.column.low_level_azshear = Some(0.008);
        let alone = analyse(vec![weak], &[], &[]);
        assert!(alone[0].fused.score < MIN_SCORE, "{}", alone[0].fused.score);
        assert!(identify(&alone, none).is_empty());
        // A report beside it makes it one, confirmed; an observed warning alone does not.
        let report = |_: f64, _: f64| Confirmation {
            observed_warning: false,
            report: Some((2.0, 3)),
        };
        assert_eq!(identify(&alone, report)[0].tier, Tier::Confirmed);
        let warned = |_: f64, _: f64| Confirmation {
            observed_warning: true,
            report: None,
        };
        assert!(identify(&alone, warned).is_empty());
    }

    #[test]
    fn a_circulation_with_no_convective_core_is_no_verdict_unless_reported() {
        // Strong, debris-backed shear, but no >= 40 dBZ core within 5 km: stratiform rain,
        // synoptic wind or clutter. No marker, at any score; a tornado report still makes one.
        let mut t = tracked(3);
        t.column.echo = None;
        let a = analyse(vec![t], &[debris_ball(0.5, 0.5)], &[]);
        assert!(a[0].fused.score >= MIN_SCORE, "{}", a[0].fused.score);
        assert!(identify(&a, |_, _| Confirmation::default()).is_empty());
        let report = |_: f64, _: f64| Confirmation {
            observed_warning: false,
            report: Some((2.0, 3)),
        };
        assert_eq!(identify(&a, report)[0].tier, Tier::Confirmed);
    }

    #[test]
    fn merged_circulations_tie_in_the_legacy_detections_once() {
        let a = analyse(vec![tracked(3)], &[debris_ball(1.0, 0.5)], &[]);
        let couplets = [couplet(east(0.5)), couplet(east(40.0))];
        let debris = [debris_ball(1.0, 0.5)];
        let circs = circulations(&a, &couplets, &debris, |_, _| Confirmation::default());
        assert_eq!(circs.len(), 1);
        let m = &circs[0].members;
        assert_eq!((circs[0].rotations(), circs[0].debris()), (1, 1), "{m:?}");
        assert_eq!(m[0].evidence, Evidence::Rotation(0), "nearest first");
    }

    #[test]
    fn far_debris_is_not_listed() {
        let a = analyse(vec![tracked(2)], &[debris_ball(20.0, 0.5)], &[]);
        assert!(a[0].debris.is_empty());
        assert!(a[0]
            .lines()
            .iter()
            .any(|l| l == "No debris signature beside it"));
        assert!(a[0].headline().contains("evidence"));
    }
}
