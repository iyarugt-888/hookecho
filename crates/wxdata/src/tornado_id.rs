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
const NO_ROTATION: &str = "No rotation found beside it";

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
    /// A letter for the marker, so the tier reads without its colour (ROADMAP_2 §13.4: state is
    /// never shown by colour alone): Possible, Likely, Debris, Confirmed.
    pub fn glyph(self) -> &'static str {
        match self {
            Tier::Possible => "P",
            Tier::Likely => "L",
            Tier::Debris => "D",
            Tier::Confirmed => "C",
        }
    }

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
            reasons.push(NO_ROTATION.into());
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

/// Detections this close (km) to a circulation's centre are the same tornado: a couplet seen
/// again a few km off at another tilt or pass, the debris ball it lofted, the parent
/// mesocyclone's rotation beside it. On the 10 December 2021 Quad-State storm the debris ball and
/// the two couplets around it sat 11–12 km apart; separate storms in a line sit further.
pub const MERGE_KM: f64 = 15.0;
/// Detections this close (km) corroborate each other when choosing a centre.
const SUPPORT_KM: f64 = 4.0;

/// One of the detections a [`Circulation`] ties together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// `couplets[i]`.
    Rotation(usize),
    /// `debris[i]`.
    Debris(usize),
}

/// A detection tied into a circulation, and where it sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Member {
    pub evidence: Evidence,
    pub lon: f64,
    pub lat: f64,
    pub confidence: f32,
    /// From the circulation's centre.
    pub km: f64,
}

/// One tornado, as one detection: the verdict for the most likely area of rotation, and every
/// rotation and debris detection around it that it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Circulation {
    /// The verdict, at the centre: the strongest rotation (or, with none, the strongest debris).
    pub id: TornadoId,
    /// Everything tied in, the centre's own detection first, then nearest first.
    pub members: Vec<Member>,
}

impl Circulation {
    pub fn rotations(&self) -> usize {
        self.members
            .iter()
            .filter(|m| matches!(m.evidence, Evidence::Rotation(_)))
            .count()
    }

    pub fn debris(&self) -> usize {
        self.members.len() - self.rotations()
    }
}

/// Everything the detectors found, as one detection per tornado rather than one marker per
/// couplet, per debris ball and per identification.
///
/// Seeds are taken best supported first, rotation before debris: the couplet with the most
/// corroboration (its score plus the detections within 4 km of it) not yet tied to a circulation
/// is the centre of a new one, and every untied couplet and debris signature within
/// [`MERGE_KM`] of it joins it. Debris with no rotation near it then seeds its own, the same way.
/// Each circulation's verdict is [`identify`] run over its own members, the strongest result
/// kept, so the tiers and reasons are exactly the ones Tornado ID gives; the score is that
/// verdict's, not a sum over members that all see the same tornado.
///
/// A circulation must have radar evidence of its own: a couplet of at least [`MIN_COUPLET`], a
/// debris signature of at least [`MIN_DEBRIS`], or a tornado report near a detection. An observed
/// tornado warning raises the tier of one that has, but cannot make one — its polygon is a county
/// wide, and every weak blob inside it would otherwise read as a confirmed tornado.
pub fn circulations(couplets: &[CoupletHit], debris: &[TdsHit]) -> Vec<Circulation> {
    // The most likely area of rotation is the couplet with the most behind it: its own score
    // plus every other detection (rotation or debris) close beside it. A strong couplet alone
    // loses to one with the debris ball on top of it.
    let support = |lon: f64, lat: f64, own: f32| {
        let near = |o: (f64, f64)| {
            let d = km((lon, lat), o);
            d > 1e-6 && d <= SUPPORT_KM
        };
        own + couplets
            .iter()
            .filter(|c| near((c.lon, c.lat)))
            .map(|c| c.confidence)
            .sum::<f32>()
            + debris
                .iter()
                .filter(|d| near((d.lon, d.lat)))
                .map(|d| d.confidence)
                .sum::<f32>()
    };
    let mut seeds: Vec<Evidence> = (0..couplets.len()).map(Evidence::Rotation).collect();
    // A tornado's own rotation is cyclonic all but rarely; an anticyclonic couplet (the
    // rear-flank shear beside it) is still tied in, but is a poor centre.
    let rot_support: Vec<f32> = couplets
        .iter()
        .map(|c| {
            let s = support(c.lon, c.lat, c.confidence);
            if c.sense == crate::rotation::Sense::Anticyclonic {
                s * 0.5
            } else {
                s
            }
        })
        .collect();
    seeds.sort_by(|a, b| {
        let (Evidence::Rotation(a), Evidence::Rotation(b)) = (*a, *b) else {
            unreachable!()
        };
        rot_support[b]
            .total_cmp(&rot_support[a])
            .then(couplets[b].confidence.total_cmp(&couplets[a].confidence))
            .then(couplets[b].vrot_ms.total_cmp(&couplets[a].vrot_ms))
    });
    let mut by_debris: Vec<usize> = (0..debris.len()).collect();
    by_debris.sort_by(|a, b| debris[*b].confidence.total_cmp(&debris[*a].confidence));
    seeds.extend(by_debris.into_iter().map(Evidence::Debris));

    let at = |e: Evidence| match e {
        Evidence::Rotation(i) => (couplets[i].lon, couplets[i].lat, couplets[i].confidence),
        Evidence::Debris(i) => (debris[i].lon, debris[i].lat, debris[i].confidence),
    };
    let mut tied_c = vec![false; couplets.len()];
    let mut tied_d = vec![false; debris.len()];
    let tied = |e: Evidence, c: &[bool], d: &[bool]| match e {
        Evidence::Rotation(i) => c[i],
        Evidence::Debris(i) => d[i],
    };
    let mut out = Vec::new();
    for seed in seeds {
        if tied(seed, &tied_c, &tied_d) {
            continue;
        }
        let (lon, lat, _) = at(seed);
        let mut members = Vec::new();
        let all = (0..couplets.len())
            .map(Evidence::Rotation)
            .chain((0..debris.len()).map(Evidence::Debris));
        for e in all {
            if tied(e, &tied_c, &tied_d) {
                continue;
            }
            let (mlon, mlat, confidence) = at(e);
            let d = km((lon, lat), (mlon, mlat));
            if e == seed || d <= MERGE_KM {
                members.push(Member {
                    evidence: e,
                    lon: mlon,
                    lat: mlat,
                    confidence,
                    km: if e == seed { 0.0 } else { d },
                });
            }
        }
        for m in &members {
            match m.evidence {
                Evidence::Rotation(i) => tied_c[i] = true,
                Evidence::Debris(i) => tied_d[i] = true,
            }
        }
        members.sort_by(|a, b| a.km.total_cmp(&b.km));
        // The verdict over this circulation's own detections.
        let cs: Vec<CoupletHit> = members
            .iter()
            .filter_map(|m| match m.evidence {
                Evidence::Rotation(i) => Some(couplets[i]),
                Evidence::Debris(_) => None,
            })
            .collect();
        let ds: Vec<TdsHit> = members
            .iter()
            .filter_map(|m| match m.evidence {
                Evidence::Debris(i) => Some(debris[i]),
                Evidence::Rotation(_) => None,
            })
            .collect();
        let own_radar = cs.iter().any(|c| c.confidence >= MIN_COUPLET)
            || ds.iter().any(|d| d.confidence >= MIN_DEBRIS)
            || cs.iter().any(|c| c.confirmation.report.is_some())
            || ds.iter().any(|d| d.confirmation.report.is_some());
        if !own_radar {
            continue;
        }
        let ids = identify(&cs, &ds);
        let Some(mut id) = ids.first().cloned() else {
            continue;
        };
        // The verdict sits at the most likely area of rotation, whichever pairing produced it.
        id.lon = lon;
        id.lat = lat;
        // Its reasons are the circulation's, not one pairing's: what the other identifications
        // inside it found is added (said once), and a debris signature's "no rotation beside it"
        // is dropped when the circulation ties rotation in.
        for other in ids.iter().skip(1) {
            for r in &other.reasons {
                if !id.reasons.contains(r) {
                    id.reasons.push(r.clone());
                }
            }
        }
        if !cs.is_empty() {
            id.reasons.retain(|r| r != NO_ROTATION);
        }
        // And its numbers are the strongest found in it.
        id.vrot_ms = cs
            .iter()
            .map(|c| c.vrot_ms)
            .fold(id.vrot_ms, |a, v| Some(a.map_or(v, |a: f32| a.max(v))));
        id.min_cc = ds
            .iter()
            .map(|d| d.min_cc)
            .fold(id.min_cc, |a, v| Some(a.map_or(v, |a: f32| a.min(v))));
        let (r, d) = (cs.len(), ds.len());
        if r + d > 1 {
            id.reasons.push(format!(
                "Ties together {r} rotation and {d} debris detection{} within {:.0} km",
                if d == 1 { "" } else { "s" },
                MERGE_KM
            ));
        }
        out.push(Circulation { id, members });
    }
    out.sort_by(|a, b| {
        b.id.tier
            .cmp(&a.id.tier)
            .then(b.id.score.total_cmp(&a.id.score))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_tornado_seen_many_ways_is_one_circulation() {
        // A strong couplet, two weaker ones within a few km (other passes, other tilts), and two
        // debris balls beside it: five markers before, one detection now.
        let cs = [
            couplet(-97.02, 0.4, 18.0, 1),
            couplet(-97.0, 0.8, 35.0, 4),
            couplet(-96.97, 0.36, 15.0, 1),
        ];
        let ds = [debris(-97.01, 0.7), debris(-96.99, 0.55)];
        let out = circulations(&cs, &ds);
        assert_eq!(out.len(), 1, "{out:?}");
        let c = &out[0];
        assert_eq!((c.rotations(), c.debris()), (3, 2));
        assert_eq!(
            (c.id.lon, c.id.lat),
            (-97.0, 35.0),
            "centred on the strongest rotation"
        );
        assert_eq!(c.members[0].evidence, Evidence::Rotation(1));
        assert_eq!(c.id.tier, Tier::Debris);
        assert!(c
            .id
            .reasons
            .last()
            .unwrap()
            .contains("3 rotation and 2 debris"));
    }

    #[test]
    fn debris_and_rotation_a_dozen_km_apart_are_one_tornado() {
        // The 10 December 2021 case: a debris ball and two couplets 11-12 km from each other.
        let out = circulations(
            &[couplet(-97.0, 0.5, 27.0, 3), couplet(-96.87, 0.45, 26.0, 2)],
            &[debris(-96.935, 0.73)],
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].members.len(), 3);
    }

    #[test]
    fn the_centre_is_the_rotation_with_the_debris_on_it() {
        // A stronger couplet alone, and a weaker one with the debris ball beside it: the centre
        // is where the evidence agrees.
        let out = circulations(
            &[couplet(-97.0, 0.7, 30.0, 3), couplet(-96.9, 0.5, 25.0, 2)],
            &[debris(-96.905, 0.7)],
        );
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].id.lon, out[0].id.lat), (-96.9, 35.0));
    }

    #[test]
    fn the_merged_verdict_speaks_for_everything_in_it() {
        let mut lone = debris(-96.95, 0.8);
        lone.unrotated = true;
        let out = circulations(&[couplet(-97.0, 0.5, 27.0, 3)], &[lone]);
        assert_eq!(out.len(), 1);
        let id = &out[0].id;
        assert!(
            !id.reasons.iter().any(|r| r == NO_ROTATION),
            "it ties rotation in: {:?}",
            id.reasons
        );
        assert!(id.reasons.iter().any(|r| r.starts_with("Rotation")));
        assert_eq!(id.vrot_ms, Some(27.0));
        assert_eq!(id.min_cc, Some(0.6));
    }

    #[test]
    fn an_anticyclonic_couplet_is_a_poor_centre() {
        let mut anti = couplet(-97.0, 0.64, 28.0, 4);
        anti.sense = crate::rotation::Sense::Anticyclonic;
        let out = circulations(&[anti, couplet(-96.9, 0.5, 26.0, 3)], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id.lon, -96.9);
    }

    #[test]
    fn every_tier_has_its_own_letter() {
        let tiers = [Tier::Possible, Tier::Likely, Tier::Debris, Tier::Confirmed];
        let mut glyphs: Vec<_> = tiers.iter().map(|t| t.glyph()).collect();
        glyphs.dedup();
        assert_eq!(glyphs.len(), tiers.len(), "tiers must differ without colour");
    }

    #[test]
    fn two_tornadoes_far_apart_stay_two() {
        // 30 km apart: a cyclic storm's old and new circulations.
        let out = circulations(
            &[couplet(-97.0, 0.8, 35.0, 3), couplet(-96.67, 0.6, 28.0, 3)],
            &[],
        );
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| c.members.len() == 1));
    }

    #[test]
    fn a_warning_alone_does_not_make_a_weak_blob_a_tornado() {
        // Weak rotation inside an observed tornado warning, far from the real one.
        let mut weak = couplet(-96.5, 0.2, 10.0, 1);
        weak.confirmation.observed_warning = true;
        let mut strong = couplet(-97.0, 0.8, 35.0, 3);
        strong.confirmation.observed_warning = true;
        let out = circulations(&[weak, strong], &[]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].id.tier, Tier::Confirmed);
        // A report beside the weak one does make it one.
        weak.confirmation.report = Some((2.0, 3));
        assert_eq!(circulations(&[weak], &[]).len(), 1);
    }

    #[test]
    fn debris_with_no_rotation_near_it_centres_its_own() {
        let out = circulations(&[couplet(-97.0, 0.5, 20.0, 2)], &[debris(-96.5, 0.7)]);
        assert_eq!(out.len(), 2);
        let lone = out.iter().find(|c| c.rotations() == 0).unwrap();
        assert_eq!((lone.id.lon, lone.id.lat), (-96.5, 35.0));
        assert_eq!(lone.id.tier, Tier::Debris);
    }

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
