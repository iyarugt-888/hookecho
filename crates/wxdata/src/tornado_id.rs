//! Tornado ID: one verdict per place a tornado may be, from the two radar detectors and the
//! human evidence, each with its reasons in words.
//!
//! [`crate::rotation`] finds velocity couplets and [`crate::tds`] finds debris balls; each already
//! scores itself, corroborates itself against the other, and carries [`crate::confirm`]'s
//! reports and observed warnings. A person should not have to read two layers and do that sum:
//! this module puts a couplet and the debris beside it into one identification, takes the
//! stronger evidence, and names a tier:
//!
//! * **Confirmed** — a tornado report near it, or an observed tornado warning over it;
//! * **Debris** — a debris signature (lofted debris means a tornado is on the ground, or was);
//! * **Likely** — strong, deep or well-scored rotation;
//! * **Possible** — rotation worth watching.
//!
//! It adds no radar evidence of its own and changes no score: the tiers are read off the
//! detectors' numbers, and the reasons say which.

use crate::confirm::Confirmation;
use crate::rotation::CoupletHit;
use crate::tds::TdsHit;

/// A couplet and a debris signature this close (km) are one identification.
pub const ASSOCIATE_KM: f64 = 4.0;
/// Below this confidence a couplet alone is not shown.
pub const MIN_COUPLET: f32 = 0.35;
/// Below this confidence a debris signature does not make the Debris tier (it still adds its
/// reason to a couplet beside it).
pub const MIN_DEBRIS: f32 = 0.5;
/// Rotational velocity (m/s, ~50 kt) that with a rooted, multi-tilt column reads as Likely.
pub const STRONG_VROT_MS: f32 = 25.0;
/// Combined score at and above which rotation alone reads as Likely.
pub const LIKELY_SCORE: f32 = 0.6;

/// How sure the identification is, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Possible,
    Likely,
    Debris,
    Confirmed,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Possible => "Tornado possible",
            Tier::Likely => "Tornado likely",
            Tier::Debris => "Tornado debris",
            Tier::Confirmed => "Tornado confirmed",
        }
    }
}

/// One identification.
#[derive(Debug, Clone, PartialEq)]
pub struct TornadoId {
    pub lon: f64,
    pub lat: f64,
    pub tier: Tier,
    /// 0..1, the stronger evidence combined with the other as independent: `1 - (1-a)(1-b)`.
    pub score: f32,
    /// Rotational velocity of the couplet, m/s.
    pub vrot_ms: Option<f32>,
    /// Lowest CC of the debris signature.
    pub min_cc: Option<f32>,
    /// Why, one line each, strongest first.
    pub reasons: Vec<String>,
}

fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dlat = (b.1 - a.1) * 111.32;
    let dlon = (b.0 - a.0) * 111.32 * a.1.to_radians().cos();
    (dlat * dlat + dlon * dlon).sqrt()
}

fn confirm_reasons(c: &Confirmation, out: &mut Vec<String>) {
    if c.level().is_none() {
        return;
    }
    // The couplet and the debris beside it carry the same evidence; say it once.
    let mut add = |line: String| {
        if !out.contains(&line) {
            out.push(line);
        }
    };
    if c.observed_warning {
        add("Inside a tornado warning that says the tornado is observed".into());
    }
    if let Some((km, _)) = c.report {
        add(format!("A tornado was reported {km:.0} km from here"));
    }
}

/// Identify tornadoes from corroborated couplets and debris signatures (as the app's rotation and
/// debris layers compute them, confirmation included). Strongest first.
pub fn identify(couplets: &[CoupletHit], debris: &[TdsHit]) -> Vec<TornadoId> {
    let mut used = vec![false; debris.len()];
    let mut out = Vec::new();
    for c in couplets {
        // The debris signature nearest this couplet, if one is close enough.
        let near = debris
            .iter()
            .enumerate()
            .filter(|(i, d)| !used[*i] && km((c.lon, c.lat), (d.lon, d.lat)) <= ASSOCIATE_KM)
            .min_by(|(_, a), (_, b)| {
                km((c.lon, c.lat), (a.lon, a.lat)).total_cmp(&km((c.lon, c.lat), (b.lon, b.lat)))
            })
            .map(|(i, d)| (i, *d));
        if c.confidence < MIN_COUPLET && near.is_none() && c.confirmation.level().is_none() {
            continue;
        }
        let mut reasons = Vec::new();
        confirm_reasons(&c.confirmation, &mut reasons);
        let mut tier = if c.confirmation.level().is_some() {
            Tier::Confirmed
        } else {
            Tier::Possible
        };
        let mut score = c.confidence;
        let mut min_cc = None;
        if let Some((i, d)) = near {
            used[i] = true;
            confirm_reasons(&d.confirmation, &mut reasons);
            if d.confirmation.level().is_some() {
                tier = Tier::Confirmed;
            }
            if d.confidence >= MIN_DEBRIS {
                tier = tier.max(Tier::Debris);
            }
            score = 1.0 - (1.0 - score) * (1.0 - d.confidence);
            min_cc = Some(d.min_cc);
            reasons.push(format!(
                "Debris signature: CC down to {:.2} in {:.0} dBZ, {} tilt{}",
                d.min_cc,
                d.max_z,
                d.tilts,
                if d.tilts == 1 { "" } else { "s" }
            ));
        }
        let deep = c.tilts >= 2 && c.rooted == Some(true);
        if score >= LIKELY_SCORE || (c.vrot_ms >= STRONG_VROT_MS && deep) {
            tier = tier.max(Tier::Likely);
        }
        reasons.push(format!(
            "Rotation {:.0} kt ({:.0} kt gate to gate), {} tilt{} from {:.1} to {:.1} km{}",
            c.vrot_ms * 1.943_844,
            c.g2g_ms * 1.943_844,
            c.tilts,
            if c.tilts == 1 { "" } else { "s" },
            c.base_km,
            c.top_km,
            if c.rooted == Some(true) {
                ", down to the lowest tilt"
            } else {
                ""
            }
        ));
        out.push(TornadoId {
            lon: c.lon,
            lat: c.lat,
            tier,
            score: score.clamp(0.0, 1.0),
            vrot_ms: Some(c.vrot_ms),
            min_cc,
            reasons,
        });
    }
    // Debris with no couplet beside it stands on its own, if it is credible or confirmed.
    for (i, d) in debris.iter().enumerate() {
        if used[i] || (d.confidence < MIN_DEBRIS && d.confirmation.level().is_none()) {
            continue;
        }
        let mut reasons = Vec::new();
        confirm_reasons(&d.confirmation, &mut reasons);
        reasons.push(format!(
            "Debris signature: CC down to {:.2} in {:.0} dBZ, {} tilt{}",
            d.min_cc,
            d.max_z,
            d.tilts,
            if d.tilts == 1 { "" } else { "s" }
        ));
        if d.unrotated {
            reasons.push("No rotation found beside it".into());
        }
        let tier = if d.confirmation.level().is_some() {
            Tier::Confirmed
        } else if d.confidence >= MIN_DEBRIS {
            Tier::Debris
        } else {
            Tier::Possible
        };
        out.push(TornadoId {
            lon: d.lon,
            lat: d.lat,
            tier,
            score: d.confidence.clamp(0.0, 1.0),
            vrot_ms: d.rotation_ms,
            min_cc: Some(d.min_cc),
            reasons,
        });
    }
    out.sort_by(|a, b| b.tier.cmp(&a.tier).then(b.score.total_cmp(&a.score)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn couplet(lon: f64, conf: f32, vrot: f32, tilts: usize) -> CoupletHit {
        CoupletHit {
            lon,
            lat: 35.0,
            vrot_ms: vrot,
            g2g_ms: vrot * 2.0,
            range_km: 40.0,
            gates: 20,
            tilts,
            top_km: 2.0,
            base_km: 0.5,
            rooted: Some(true),
            sense: crate::rotation::Sense::Cyclonic,
            debris_confidence: None,
            confidence: conf,
            confirmation: Confirmation::default(),
        }
    }

    fn debris(lon: f64, conf: f32) -> TdsHit {
        TdsHit {
            lon,
            lat: 35.0,
            gates: 12,
            min_cc: 0.6,
            mean_cc: 0.7,
            mean_z: 50.0,
            max_z: 56.0,
            area_km2: 4.0,
            contrast: Some(0.2),
            range_km: 40.0,
            tilts: 2,
            top_km: 1.5,
            base_km: 0.5,
            rooted: Some(true),
            rotation_ms: None,
            unrotated: false,
            zdr_db: None,
            confirmation: Confirmation::default(),
            confidence: conf,
        }
    }

    #[test]
    fn a_couplet_with_debris_beside_it_is_one_debris_identification() {
        let ids = identify(&[couplet(-97.0, 0.5, 20.0, 2)], &[debris(-97.01, 0.7)]);
        assert_eq!(ids.len(), 1, "{ids:?}");
        assert_eq!(ids[0].tier, Tier::Debris);
        assert!(
            ids[0].score > 0.8,
            "combined, not the larger alone: {}",
            ids[0].score
        );
        assert_eq!(ids[0].reasons.len(), 2);
        // An observed warning over both is one reason, not two.
        let (mut c, mut d) = (couplet(-97.0, 0.5, 20.0, 2), debris(-97.01, 0.7));
        c.confirmation.observed_warning = true;
        d.confirmation.observed_warning = true;
        let ids = identify(&[c], &[d]);
        assert_eq!(ids[0].tier, Tier::Confirmed);
        assert_eq!(ids[0].reasons.len(), 3, "{:?}", ids[0].reasons);
    }

    #[test]
    fn tiers_follow_the_evidence() {
        // Weak rotation: not shown. Moderate: possible. Strong and deep: likely.
        assert!(identify(&[couplet(-97.0, 0.2, 12.0, 1)], &[]).is_empty());
        assert_eq!(
            identify(&[couplet(-97.0, 0.4, 15.0, 1)], &[])[0].tier,
            Tier::Possible
        );
        assert_eq!(
            identify(&[couplet(-97.0, 0.4, 30.0, 3)], &[])[0].tier,
            Tier::Likely
        );
        // Debris far from any couplet stands alone.
        let ids = identify(&[couplet(-97.0, 0.4, 15.0, 1)], &[debris(-96.0, 0.6)]);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].tier, Tier::Debris, "strongest tier first");
        // A report makes it confirmed, whatever the radar says.
        let mut c = couplet(-97.0, 0.3, 10.0, 1);
        c.confirmation.report = Some((3.0, 5));
        let ids = identify(&[c], &[]);
        assert_eq!(ids[0].tier, Tier::Confirmed);
        assert!(ids[0].reasons[0].contains("reported"));
    }
}
