//! One fusion layer: every line of radar evidence about a circulation, counted once, combined into
//! one explainable evidence score (detectionplan.md Phases 7 and 8, stage A).
//!
//! A tracked LLSD rotation column ([`crate::rotation_tracks::Tracked`]) is the unit. Its
//! [`Features`] gather what the earlier stages measured, each once: low-level and peak shear,
//! depth, tilts, rooting, sense, persistence and trend from the track, and the polarimetric
//! evidence of a tornado debris signature beside it ([`crate::debris_class`]) with its hail signs.
//! Reports and warnings are not features: the score is radar alone, so it can be verified against
//! them.
//!
//! The score is logistic: `1 / (1 + e^-(bias + Σ wᵢ·fᵢ))`. Each `wᵢ·fᵢ` is a named [`Term`] in
//! log-odds, so [`Fused::terms`] reproduce the score exactly and say what raised or lowered it.
//! The weights ([`WEIGHTS`]) are fitted offline to the backtest corpus by
//! `scripts/fusion/fit.py`, held out by whole event. With a corpus this small the score is an
//! evidence score, not a calibrated probability, and must never be shown as one.

use crate::debris_class::{association_radius_km, DebrisAssessment, DebrisClass, DebrisParams};
use crate::rotation::Sense;
use crate::rotation_tracks::Tracked;
use crate::tds::TdsHit;

/// Version of the features and weights, recorded with anything derived from them.
pub const ALGORITHM_VERSION: &str = "fusion-2";

/// The features, in [`Features::values`] order. Shear is in units of 0.01 s⁻¹.
pub const FEATURE_NAMES: [&str; 13] = [
    "low_level_shear",
    "max_shear",
    "depth_km",
    "tilts",
    "rooted",
    "cyclonic",
    "persisted_2",
    "persisted_3",
    "shear_trend",
    "debris",
    "debris_hail",
    "range_100km",
    "weak_echo_root",
];

/// What the fusion knows about one circulation in one volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Features {
    /// Strongest 0-2 km peak AzShear, 0.01 s⁻¹ (0 with no member there).
    pub low_level_shear: f32,
    /// Strongest peak AzShear of any member, 0.01 s⁻¹.
    pub max_shear: f32,
    pub depth_km: f32,
    pub tilts: f32,
    /// 1 when a member is on the lowest tilt.
    pub rooted: f32,
    /// 1 when it turns the way this hemisphere's tornadoes do.
    pub cyclonic: f32,
    /// 1 once the track has been seen in 2 or more volumes, and (`persisted_3`) in 3 or more: the
    /// plan's step shape, "1 volume neutral, 2 meaningful, 3+ strong", rather than a straight
    /// line. A new circulation loses nothing here: strong shear and debris beside it can carry
    /// the score alone (the plan's escape path for a rapidly developing tornado).
    pub persisted_2: f32,
    pub persisted_3: f32,
    /// Peak AzShear trend, 0.01 s⁻¹ per 10 minutes, clamped to ±3 (0 for a new track).
    pub shear_trend: f32,
    /// Polarimetric evidence (0..1) of the strongest tornado debris signature within the
    /// association radius; 0 with none.
    pub debris: f32,
    /// 1 when that debris signature carries a hail sign.
    pub debris_hail: f32,
    /// Range of the column's base, in 100 km.
    pub range_100km: f32,
    /// 1 when the column reaches the lowest tilt only through an object found in weak echo.
    pub weak_echo_root: f32,
}

impl Features {
    /// The features of a tracked column, given the volume's classified debris signatures.
    pub fn of(t: &Tracked, debris: &[(TdsHit, DebrisAssessment)]) -> Features {
        let c = &t.column;
        let base = c.members.first();
        let range_km = base.map_or(0.0, |m| m.object.range_km);
        let p = DebrisParams::default();
        let near = debris
            .iter()
            .filter(|(h, a)| {
                a.class == DebrisClass::TornadoDebrisSignature
                    && crate::tds::ground_km((h.lon, h.lat), (c.lon, c.lat))
                        <= association_radius_km(h.range_km, &p) as f64
            })
            .max_by(|a, b| a.1.polarimetric.total_cmp(&b.1.polarimetric));
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        Features {
            low_level_shear: c.low_level_azshear.unwrap_or(0.0) * 100.0,
            max_shear: c.max_azshear * 100.0,
            depth_km: c.depth_km,
            tilts: c.tilts() as f32,
            rooted: flag(c.rooted),
            cyclonic: flag(c.sense == Sense::Cyclonic),
            persisted_2: flag(t.age_volumes >= 2),
            persisted_3: flag(t.age_volumes >= 3),
            shear_trend: t
                .azshear_trend
                .map_or(0.0, |s| (s * 100.0).clamp(-3.0, 3.0)),
            debris: near.map_or(0.0, |(_, a)| a.polarimetric),
            debris_hail: flag(near.is_some_and(|(_, a)| !a.hail_signs.is_empty())),
            range_100km: range_km / 100.0,
            weak_echo_root: flag(
                c.rooted
                    && c.members
                        .iter()
                        .filter(|m| m.tilt == 0)
                        .all(|m| m.weak_echo),
            ),
        }
    }

    /// The features in [`FEATURE_NAMES`] order.
    pub fn values(&self) -> [f32; 13] {
        [
            self.low_level_shear,
            self.max_shear,
            self.depth_km,
            self.tilts,
            self.rooted,
            self.cyclonic,
            self.persisted_2,
            self.persisted_3,
            self.shear_trend,
            self.debris,
            self.debris_hail,
            self.range_100km,
            self.weak_echo_root,
        ]
    }
}

/// Logistic weights: the bias, then one per feature in [`FEATURE_NAMES`] order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub bias: f32,
    pub w: [f32; 13],
}

/// The fitted weights (`scripts/fusion/fit.py`, first fit, on the 9-event backtest corpus of
/// `docs/backtest-events.txt`).
///
/// Each weight is held to the sign physics expects, and range is left out. A free fit learned the
/// corpus's quirks instead: range got the largest weight, negative, from where this corpus's
/// reports happen to be (LLSD underestimates shear far out, so physically the same measured shear
/// means more rotation there); hail beside debris counted *for* a tornado; and the two correlated
/// shear features took opposite signs. Shear trend, hail and weak-echo rooting came out with the
/// wrong sign under the constraint and are dropped (zero).
///
/// Held out by whole event, the constrained fit ranks 29, 59 and 102 verified rows among its top
/// 50, 100 and 200 of 7783, against 26, 46 and 85 for peak shear alone and 23, 50 and 99 for the
/// free fit. Over every row its AUC is lower (0.68, against 0.70 and 0.73): it is better where
/// the score is high, not at telling weak rows apart. Its top 50 rows reach 5 of the 8 tornadic
/// events, against 7 for peak shear alone. Debris beside a column carries the most weight.
pub const WEIGHTS: Weights = Weights {
    bias: -4.1732,
    w: [
        0.2139, // low_level_shear
        0.6968, // max_shear
        0.1165, // depth_km
        0.1789, // tilts
        0.0203, // rooted
        0.0136, // cyclonic
        // fusion-1 fitted one weight per volume of persistence (0.1249, capped at 3); until the
        // step features are refitted, each step carries one of those.
        0.1249, // persisted_2
        0.1249, // persisted_3
        0.0,    // shear_trend
        2.4257, // debris
        0.0,    // debris_hail
        0.0,    // range_100km
        0.0,    // weak_echo_root
    ],
};

/// One named contribution to a fused score, in log-odds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Term {
    pub label: &'static str,
    pub logit: f32,
}

/// A fused evidence score and what it is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    /// 0..1 evidence score (not a probability): the logistic of the terms' sum.
    pub score: f32,
    /// The bias first, then every feature's contribution, in [`FEATURE_NAMES`] order.
    pub terms: Vec<Term>,
}

/// Fuse one circulation's features with `w`.
pub fn fuse(f: &Features, w: &Weights) -> Fused {
    let mut terms = vec![Term {
        label: "baseline",
        logit: w.bias,
    }];
    terms.extend(
        FEATURE_NAMES
            .iter()
            .zip(f.values())
            .zip(w.w)
            .map(|((label, x), wi)| Term {
                label,
                logit: wi * x,
            }),
    );
    let z: f32 = terms.iter().map(|t| t.logit).sum();
    Fused {
        score: 1.0 / (1.0 + (-z).exp()),
        terms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features() -> Features {
        Features {
            low_level_shear: 2.5,
            max_shear: 3.0,
            depth_km: 1.2,
            tilts: 3.0,
            rooted: 1.0,
            cyclonic: 1.0,
            persisted_2: 1.0,
            persisted_3: 1.0,
            shear_trend: 0.5,
            debris: 0.7,
            debris_hail: 0.0,
            range_100km: 0.4,
            weak_echo_root: 0.0,
        }
    }

    #[test]
    fn the_terms_reproduce_the_score_exactly() {
        let w = Weights {
            bias: -3.0,
            w: [
                0.8, 0.2, 0.3, 0.1, 0.9, 1.1, 0.4, 0.2, 0.2, 2.0, -0.8, -0.5, -0.3,
            ],
        };
        let f = fuse(&features(), &w);
        assert_eq!(f.terms.len(), 14);
        let z: f32 = f.terms.iter().map(|t| t.logit).sum();
        assert_eq!(f.score, 1.0 / (1.0 + (-z).exp()));
        assert_eq!(f.terms[10].label, "debris");
        assert!((f.terms[10].logit - 1.4).abs() < 1e-6);
    }

    #[test]
    fn more_evidence_with_positive_weights_never_lowers_the_score() {
        let w = Weights {
            bias: -2.0,
            w: [0.5; 13],
        };
        let base = fuse(&features(), &w).score;
        let mut more = features();
        more.debris = 0.9;
        assert!(fuse(&more, &w).score > base);
    }

    #[test]
    fn the_fitted_weights_point_the_way_physics_does() {
        // Every feature is evidence for a tornado or left out, never evidence against: the
        // constraint `fit.py` holds the fit to.
        assert!(WEIGHTS.w.iter().all(|w| *w >= 0.0), "{:?}", WEIGHTS.w);
        let i = |n: &str| FEATURE_NAMES.iter().position(|f| *f == n).unwrap();
        assert_eq!(WEIGHTS.w[i("range_100km")], 0.0, "range is not evidence");
    }

    #[test]
    fn debris_on_deep_persistent_rotation_outranks_a_weak_new_blip() {
        let strong = fuse(&features(), &WEIGHTS);
        let blip = fuse(
            &Features {
                low_level_shear: 0.7,
                max_shear: 0.7,
                depth_km: 0.0,
                tilts: 1.0,
                rooted: 1.0,
                cyclonic: 1.0,
                persisted_2: 0.0,
                persisted_3: 0.0,
                shear_trend: 0.0,
                debris: 0.0,
                debris_hail: 0.0,
                range_100km: 0.4,
                weak_echo_root: 0.0,
            },
            &WEIGHTS,
        );
        assert!(
            strong.score > 0.5 && blip.score < 0.1,
            "{} vs {}",
            strong.score,
            blip.score
        );
    }
}
