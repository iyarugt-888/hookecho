//! The new rotation pipeline as an analyst sees it (detectionplan.md Phases 12 and 13).
//!
//! [`analyse`] takes one volume's tracked LLSD rotation columns ([`crate::rotation_tracks`]) and its
//! debris signatures, classifies the debris ([`crate::debris_class`]), fuses each column
//! ([`crate::tornado_fusion`]) and returns them strongest first. [`Analysed::lines`] says why each
//! one exists: every member tilt's measurements and quality, the track, the debris beside it, and
//! every term of its evidence score. A false alarm can then be diagnosed rather than only seen.
//!
//! This pipeline runs beside the app's own rotation, debris and Tornado ID detectors and never
//! replaces them: it raises no alert and feeds nothing else, until it is validated (Phase 13).

use crate::debris_class::{
    association_radius_km, classify, DebrisAssessment, DebrisParams, HailSign,
};
use crate::rotation::Sense;
use crate::rotation_tracks::Tracked;
use crate::tds::TdsHit;
use crate::tornado_fusion::{fuse, Features, Fused, WEIGHTS};

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

/// Compass bearing (degrees from north) of motion `(east, north)`.
fn bearing(u: f32, v: f32) -> f32 {
    u.atan2(v).to_degrees().rem_euclid(360.0)
}

fn opt(v: Option<f32>, fmt: impl Fn(f32) -> String) -> String {
    v.map_or("none".into(), fmt)
}

impl Analysed {
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
