//! What a low-CC signature is: a polarimetric anomaly, a debris candidate, or a tornado debris
//! signature (detectionplan.md Phase 6).
//!
//! [`crate::tds`] finds connected regions of low correlation coefficient in strong echo and scores
//! them on their own polarimetric evidence ([`TdsHit::raw_confidence`]: how far CC falls, the
//! contrast with the CC around it, the echo, the size, ZDR, the vertical extent). That evidence
//! says *something* is scattering incoherently; it does not say what. Large hail, a melting layer,
//! wet snow, biological scatter and clutter all lower CC in strong echo. Lofted debris is the one
//! that comes with a tornado's rotation. So:
//!
//! * **[`DebrisClass::PolarimetricAnomaly`]**: the polarimetric evidence alone is not credible
//!   ([`DebrisParams::min_polarimetric`]), or it is credible but hail-like (see below) with no
//!   rotation beside it;
//! * **[`DebrisClass::DebrisCandidate`]**: credible on its own evidence, but no credible low-level
//!   rotation beside it;
//! * **[`DebrisClass::TornadoDebrisSignature`]**: credible, with a cyclonic LLSD rotation column
//!   ([`crate::rotation_columns`]) whose low-level shear reaches [`DebrisParams::min_low_level_s`]
//!   within the association radius.
//!
//! The radius adapts to the beam ([`association_radius_km`]): it is a fixed allowance plus two beam
//! widths, because a ball and the shear that lofted it are each smeared across the beam. On the
//! backtest corpus verified debris sat a median 3.9 km from the nearest cyclonic column (85% of
//! credible ones within 5 km), unverified debris 11.6 km.
//!
//! **Hail.** A hailstorm rotates too: the 8 May 2017 Denver hailstorm (KFTG) has credible low-CC
//! cores 3 km from cyclonic columns. Hail signs recorded here are negative ZDR (≤
//! [`DebrisParams::hail_zdr_db`]: large tumbling hail; Denver's median was −0.04 dB and its lowest
//! tenth below −1.1 dB, where verified debris had a median of +1.2 dB) and a MEHS/POSH hail core
//! close by, when the caller has them. Hail signs keep an unrotated signature an anomaly; with
//! rotation beside it the signature is still a debris signature, with its hail signs kept for the
//! fusion to weigh (a tornado can carry hail beside it). Very high reflectivity is deliberately not
//! a hail sign: on the corpus, verified debris balls had *higher* peak reflectivity (median 61 dBZ)
//! than Denver's hail anomalies (53 dBZ).

use crate::rotation::Sense;
use crate::rotation_columns::RotationColumn;
use crate::tds::TdsHit;

/// Version of the classification rules, recorded with anything derived from them.
pub const ALGORITHM_VERSION: &str = "debris-class-1";

/// WSR-88D half-power beam width, degrees.
pub const BEAM_WIDTH_DEG: f32 = 0.95;

/// How signatures are classified. Provisional, for the backtest to tune.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebrisParams {
    /// Least polarimetric evidence ([`TdsHit::raw_confidence`]) of a credible signature.
    pub min_polarimetric: f32,
    /// Least low-level (0-2 km) peak AzShear (s⁻¹) of a column that can promote a signature.
    pub min_low_level_s: f32,
    /// The radius's fixed allowance (km), before the beam widths.
    pub base_radius_km: f32,
    /// Beam widths added to the radius.
    pub beam_widths: f32,
    /// Mean ZDR (dB) at or below which a signature is hail-like.
    pub hail_zdr_db: f32,
    /// A hail core with POSH at least this (0..1) this close (km) is a hail sign.
    pub hail_posh: f32,
    pub hail_core_km: f32,
}

impl Default for DebrisParams {
    fn default() -> Self {
        DebrisParams {
            min_polarimetric: 0.5,
            min_low_level_s: 0.01,
            base_radius_km: 3.0,
            beam_widths: 2.0,
            hail_zdr_db: -0.5,
            hail_posh: 0.5,
            hail_core_km: 3.0,
        }
    }
}

/// What a low-CC signature is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DebrisClass {
    PolarimetricAnomaly,
    DebrisCandidate,
    TornadoDebrisSignature,
}

impl DebrisClass {
    pub fn label(self) -> &'static str {
        match self {
            DebrisClass::PolarimetricAnomaly => "polarimetric anomaly",
            DebrisClass::DebrisCandidate => "debris candidate",
            DebrisClass::TornadoDebrisSignature => "tornado debris signature",
        }
    }
}

/// A sign that a signature is hail rather than debris.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HailSign {
    /// Mean ZDR (dB) at or below [`DebrisParams::hail_zdr_db`].
    NegativeZdr(f32),
    /// A hail core of this POSH (0..1) this far away (km).
    HailCore { posh: f32, km: f32 },
}

/// The rotation column that promotes a signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationLink {
    /// From the signature to the column's base, km.
    pub km: f32,
    pub low_level_azshear: f32,
    pub tilts: usize,
    pub rooted: bool,
    pub depth_km: f32,
}

/// One signature classified.
#[derive(Debug, Clone, PartialEq)]
pub struct DebrisAssessment {
    pub class: DebrisClass,
    /// The signature's own polarimetric evidence, 0..1 ([`TdsHit::raw_confidence`]).
    pub polarimetric: f32,
    pub hail_signs: Vec<HailSign>,
    /// The nearest qualifying column within the radius, if any.
    pub rotation: Option<RotationLink>,
    /// The association radius used at this signature's range, km.
    pub radius_km: f32,
}

/// The distance (km) within which a rotation column can promote a signature at `range_km`: the
/// fixed allowance plus [`DebrisParams::beam_widths`] beam widths there.
pub fn association_radius_km(range_km: f32, p: &DebrisParams) -> f32 {
    p.base_radius_km + p.beam_widths * range_km * BEAM_WIDTH_DEG.to_radians()
}

/// Classify one signature. `columns` are the volume's LLSD rotation columns (any; only cyclonic
/// ones with enough low-level shear count) and `hail_cores` the volume's hail cores as
/// `(lon, lat, posh 0..1)`, empty if the caller has none.
pub fn classify(
    hit: &TdsHit,
    columns: &[RotationColumn],
    hail_cores: &[(f64, f64, f32)],
    p: &DebrisParams,
) -> DebrisAssessment {
    let radius_km = association_radius_km(hit.range_km, p);
    let here = (hit.lon, hit.lat);
    let rotation = columns
        .iter()
        .filter(|c| {
            c.sense == Sense::Cyclonic
                && c.low_level_azshear.is_some_and(|s| s >= p.min_low_level_s)
        })
        .map(|c| (crate::tds::ground_km(here, (c.lon, c.lat)) as f32, c))
        .filter(|(d, _)| *d <= radius_km)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(km, c)| RotationLink {
            km,
            low_level_azshear: c.low_level_azshear.unwrap_or(0.0),
            tilts: c.tilts(),
            rooted: c.rooted,
            depth_km: c.depth_km,
        });
    let mut hail_signs = Vec::new();
    if let Some(z) = hit.zdr_db.filter(|z| *z <= p.hail_zdr_db) {
        hail_signs.push(HailSign::NegativeZdr(z));
    }
    if let Some((posh, km)) = hail_cores
        .iter()
        .filter(|h| h.2 >= p.hail_posh)
        .map(|h| (h.2, crate::tds::ground_km(here, (h.0, h.1)) as f32))
        .filter(|(_, km)| *km <= p.hail_core_km)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    {
        hail_signs.push(HailSign::HailCore { posh, km });
    }
    let polarimetric = hit.raw_confidence;
    let class = if polarimetric < p.min_polarimetric {
        DebrisClass::PolarimetricAnomaly
    } else if rotation.is_some() {
        DebrisClass::TornadoDebrisSignature
    } else if !hail_signs.is_empty() {
        DebrisClass::PolarimetricAnomaly
    } else {
        DebrisClass::DebrisCandidate
    };
    DebrisAssessment {
        class,
        polarimetric,
        hail_signs,
        rotation,
        radius_km,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confirm::Confirmation;

    fn east(km: f64) -> f64 {
        -97.0 + km / (111.32 * 35f64.to_radians().cos())
    }

    fn hit(raw: f32, zdr: Option<f32>, range_km: f32) -> TdsHit {
        TdsHit {
            lon: east(0.0),
            lat: 35.0,
            gates: 20,
            min_cc: 0.3,
            mean_cc: 0.6,
            mean_z: 50.0,
            max_z: 58.0,
            area_km2: 3.0,
            contrast: Some(0.2),
            range_km,
            tilts: 2,
            top_km: 1.0,
            base_km: 0.4,
            rooted: Some(true),
            rotation_ms: None,
            unrotated: false,
            zdr_db: zdr,
            confirmation: Confirmation::default(),
            raw_confidence: raw,
            confidence: raw,
        }
    }

    fn column(km: f64, low: f32, sense: Sense) -> RotationColumn {
        RotationColumn {
            lon: east(km),
            lat: 35.0,
            sense,
            members: Vec::new(),
            base_km: 0.4,
            top_km: 1.4,
            depth_km: 1.0,
            rooted: true,
            low_level_azshear: Some(low),
            transition_azshear: None,
            mid_level_azshear: None,
            max_azshear: low,
            integrated_azshear: 0.0,
            lean_km_per_km: None,
            lean_bearing_deg: None,
            echo: None,
        }
    }

    #[test]
    fn credible_debris_on_strong_low_level_rotation_is_a_tds() {
        let p = DebrisParams::default();
        let a = classify(
            &hit(0.7, Some(0.5), 30.0),
            &[column(2.0, 0.02, Sense::Cyclonic)],
            &[],
            &p,
        );
        assert_eq!(a.class, DebrisClass::TornadoDebrisSignature);
        let r = a.rotation.unwrap();
        assert!((r.km - 2.0).abs() < 0.05 && r.rooted && r.tilts == 0);
    }

    #[test]
    fn credible_debris_without_rotation_is_only_a_candidate() {
        let p = DebrisParams::default();
        // Rotation too far, too weak, or turning the wrong way does not count.
        for cols in [
            vec![column(9.0, 0.02, Sense::Cyclonic)],
            vec![column(1.0, 0.006, Sense::Cyclonic)],
            vec![column(1.0, 0.02, Sense::Anticyclonic)],
            vec![],
        ] {
            let a = classify(&hit(0.7, Some(0.5), 30.0), &cols, &[], &p);
            assert_eq!(a.class, DebrisClass::DebrisCandidate, "{cols:?}");
            assert!(a.rotation.is_none());
        }
    }

    #[test]
    fn weak_polarimetric_evidence_is_an_anomaly_whatever_rotates_beside_it() {
        let a = classify(
            &hit(0.3, Some(0.5), 30.0),
            &[column(1.0, 0.03, Sense::Cyclonic)],
            &[],
            &DebrisParams::default(),
        );
        assert_eq!(a.class, DebrisClass::PolarimetricAnomaly);
    }

    #[test]
    fn a_hail_signature_with_no_rotation_is_not_debris() {
        let p = DebrisParams::default();
        // Strong low-CC evidence, negative ZDR: hail.
        let a = classify(&hit(0.8, Some(-1.2), 30.0), &[], &[], &p);
        assert_eq!(a.class, DebrisClass::PolarimetricAnomaly);
        assert_eq!(a.hail_signs, vec![HailSign::NegativeZdr(-1.2)]);
        // Or a hail core right there.
        let a = classify(
            &hit(0.8, Some(0.4), 30.0),
            &[],
            &[(east(1.0), 35.0, 0.9)],
            &p,
        );
        assert_eq!(a.class, DebrisClass::PolarimetricAnomaly);
        assert!(matches!(a.hail_signs[0], HailSign::HailCore { .. }));
        // With a tornado's rotation beside it, it is still debris, and the hail is on record.
        let a = classify(
            &hit(0.8, Some(-1.2), 30.0),
            &[column(1.5, 0.03, Sense::Cyclonic)],
            &[],
            &p,
        );
        assert_eq!(a.class, DebrisClass::TornadoDebrisSignature);
        assert_eq!(a.hail_signs.len(), 1);
    }

    #[test]
    fn the_radius_widens_with_the_beam() {
        let p = DebrisParams::default();
        let near = association_radius_km(20.0, &p);
        let far = association_radius_km(120.0, &p);
        assert!((near - 3.66).abs() < 0.02, "{near}");
        assert!((far - 6.98).abs() < 0.02, "{far}");
        // 5 km away: too far at 20 km range, close enough at 120 km.
        let col = [column(5.0, 0.02, Sense::Cyclonic)];
        assert_eq!(
            classify(&hit(0.7, None, 20.0), &col, &[], &p).class,
            DebrisClass::DebrisCandidate
        );
        assert_eq!(
            classify(&hit(0.7, None, 120.0), &col, &[], &p).class,
            DebrisClass::TornadoDebrisSignature
        );
    }
}
