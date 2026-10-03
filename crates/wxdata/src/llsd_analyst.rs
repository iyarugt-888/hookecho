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
    identify_with(analysed, confirm, None)
}

/// [`identify`], with the opt-in rotation-only bar of [`Analysed::tornado_id_with`].
pub fn identify_with(
    analysed: &[Analysed],
    confirm: impl Fn(f64, f64) -> Confirmation,
    rotation_only_possible: Option<f32>,
) -> Vec<TornadoId> {
    let mut out: Vec<TornadoId> = analysed
        .iter()
        .filter_map(|a| {
            let c = &a.tracked.column;
            a.tornado_id_with(&confirm(c.lon, c.lat), rotation_only_possible)
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
    circulations_with(analysed, couplets, debris, confirm, None)
}

/// [`circulations`], with the opt-in rotation-only bar of [`Analysed::tornado_id_with`].
pub fn circulations_with(
    analysed: &[Analysed],
    couplets: &[CoupletHit],
    debris: &[TdsHit],
    confirm: impl Fn(f64, f64) -> Confirmation,
    rotation_only_possible: Option<f32>,
) -> Vec<Circulation> {
    let ids = identify_with(analysed, confirm, rotation_only_possible);
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
    /// evidence is under [`MIN_SCORE`] and no tornado report confirms it: an observed warning
    /// raises the tier of a detection but cannot make one (its polygon is a county wide).
    pub fn tornado_id(&self, confirmation: &Confirmation) -> Option<TornadoId> {
        self.tornado_id_with(confirmation, None)
    }

    /// [`Self::tornado_id`], with an opt-in bar (s⁻¹): a rooted, cyclonic column with no tornado
    /// debris signature beside it reaches Possible once its 0-2 km shear is at least the bar,
    /// whatever its evidence score. Its score is unchanged and its first reason says why it is
    /// shown.
    ///
    /// Off by default. Rotation-only tornadoes with strong low-level shear (0.017-0.028 s⁻¹ on the
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
            });
        if score < MIN_SCORE && confirmation.report.is_none() && !lifted {
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
                 at or above the {:.3} s\u{207b}\u{b9} you set); its evidence score alone is \
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
                echo: None,
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
    fn strong_rotation_without_debris_reaches_possible_only_when_opted_in() {
        // One volume, no debris: strong low-level shear (0.03 s^-1) that fuses under Possible.
        let mut t = tracked(1);
        t.column.low_level_azshear = Some(0.021);
        t.column.max_azshear = 0.021;
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
        let mut aloft = t.clone();
        aloft.column.rooted = false;
        let aloft = &analyse(vec![aloft], &[], &[])[0];
        assert!(aloft.tornado_id_with(&none, Some(0.020)).is_none());
        // identify_with passes the bar through.
        assert_eq!(
            identify_with(&analyse(vec![t], &[], &[]), |_, _| none, Some(0.020)).len(),
            1
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
