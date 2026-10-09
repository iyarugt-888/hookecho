//! Every low-level pass of a volume (detectionplan.md, "Every low-level pass"). Under SAILS or
//! MRLE the radar revisits its lowest tilt partway through the volume: 68% of the backtest's
//! volumes have two to four velocity passes there, a median 1.8 minutes apart in volumes 5.7
//! minutes long. The Tornado ID's columns are built and tracked at each of them, each pass at the
//! lowest tilt under the volume's own upper tilts, so a tornado is marked at the pass that first
//! shows it rather than at the volume's last. On four random tornado samples that found 8-10
//! points more of the tornadoes and moved the first marker's median lead from +0 to +2 minutes to
//! +3 to +5.
//!
//! [`passes`] lists them; [`at_pass`] gives the sweeps one pass reads.

use crate::level2::{self, BinnedSweep, Moment, Scan};
use chrono::{DateTime, Utc};

/// One velocity pass at the lowest tilt: which sweep it is, and when it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    pub sweep: usize,
    pub time: DateTime<Utc>,
}

/// The lowest tilt's velocity passes of `scan`, oldest first, from `start` (the volume's own time)
/// on. The last is the one [`level2::bin_scan`] reads.
pub fn passes(scan: &Scan, start: DateTime<Utc>) -> Vec<Pass> {
    level2::moment_cuts(scan, Moment::Velocity, 0)
        .into_iter()
        .filter_map(|(sweep, ms)| {
            let time = DateTime::from_timestamp_millis(ms)?;
            (time >= start).then_some(Pass { sweep, time })
        })
        .collect()
}

/// Which of a volume's lowest-tilt-first sweep lists start with the lowest tilt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lowest {
    pub velocity: bool,
    pub dual_pol: bool,
    pub zdr: bool,
}

/// The sweeps one pass reads, in the shapes the detectors take.
#[derive(Debug, Clone)]
pub struct PassInputs {
    /// (dealiased velocity, reflectivity), lowest tilt first: for the rotation columns.
    pub velocity: Vec<(BinnedSweep, BinnedSweep)>,
    /// (reflectivity, correlation coefficient), lowest tilt first: for the debris signatures.
    pub dual_pol: Vec<(BinnedSweep, BinnedSweep)>,
    /// Differential reflectivity, lowest tilt first.
    pub zdr: Vec<BinnedSweep>,
}

/// The sweeps `pass` reads: its own velocity at the lowest tilt, with the reflectivity, CC and
/// ZDR passes that finished nearest it, under the volume's own upper tilts (`velocity`,
/// `dual_pol`, `zdr`, lowest first, the lowest present where `lowest` says). `None` when the
/// pass or its reflectivity cannot be binned, or the volume has no lowest velocity tilt to replace.
pub fn at_pass(
    scan: &Scan,
    pass: &Pass,
    velocity: &[(BinnedSweep, BinnedSweep)],
    dual_pol: &[(BinnedSweep, BinnedSweep)],
    zdr: &[BinnedSweep],
    lowest: Lowest,
) -> Option<PassInputs> {
    at_pass_with(scan, pass, velocity, dual_pol, zdr, lowest, false)
}

/// [`at_pass`], with `same_cut` pairing the lowest tilt's CC with the reflectivity of its own cut
/// (where that cut carries reflectivity) rather than the reflectivity beside the velocity pass,
/// so the debris detector reads two moments scanned together ([`level2::bin_scan_from_cut_of`]).
pub fn at_pass_with(
    scan: &Scan,
    pass: &Pass,
    velocity: &[(BinnedSweep, BinnedSweep)],
    dual_pol: &[(BinnedSweep, BinnedSweep)],
    zdr: &[BinnedSweep],
    lowest: Lowest,
    same_cut: bool,
) -> Option<PassInputs> {
    if !lowest.velocity || velocity.is_empty() {
        return None;
    }
    let ms = pass.time.timestamp_millis();
    let nearest = |m: Moment| {
        level2::nearest_cut(scan, m, 0, ms)
            .and_then(|i| level2::bin_sweep_index(scan, m, i, false).ok())
    };
    let vel = level2::bin_sweep_index(scan, Moment::Velocity, pass.sweep, true).ok()?;
    let z = nearest(Moment::Reflectivity)?;
    let mut out = PassInputs {
        velocity: velocity.to_vec(),
        dual_pol: dual_pol.to_vec(),
        zdr: zdr.to_vec(),
    };
    out.velocity[0] = (vel, z.clone());
    if lowest.dual_pol && !out.dual_pol.is_empty() {
        let cc_cut = level2::nearest_cut(scan, Moment::CorrelationCoefficient, 0, ms);
        let cc = cc_cut.and_then(|i| {
            level2::bin_sweep_index(scan, Moment::CorrelationCoefficient, i, false).ok()
        });
        let z = match cc_cut.filter(|_| same_cut) {
            Some(i) => level2::bin_sweep_index(scan, Moment::Reflectivity, i, false).unwrap_or(z),
            None => z,
        };
        match cc {
            Some(cc) => out.dual_pol[0] = (z, cc),
            None => {
                out.dual_pol.remove(0);
            }
        }
    }
    if lowest.zdr && !out.zdr.is_empty() {
        match nearest(Moment::DifferentialReflectivity) {
            Some(zd) => out.zdr[0] = zd,
            None => {
                out.zdr.remove(0);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexrad_model::data::{MomentData, Radial, RadialStatus, Sweep};

    fn vcp() -> nexrad_model::data::VolumeCoveragePattern {
        use nexrad_model::data::{PulseWidth, VolumeCoveragePattern};
        VolumeCoveragePattern::new(
            212,
            0,
            0.5,
            PulseWidth::Short,
            false,
            0,
            false,
            0,
            false,
            false,
            0,
            false,
            false,
            Vec::new(),
        )
    }

    /// A sweep at `elevation` (its `number` in the volume) scanned at `ms`: eight radials of
    /// reflectivity and velocity, every gate's velocity code `vel`.
    fn sweep(number: u8, elevation: f32, ms: i64, vel: u8) -> Sweep {
        let radials = (0..8)
            .map(|i| {
                let gates = 40u16;
                let z = MomentData::from_fixed_point(gates, 2125, 250, 8, 2.0, 66.0, vec![150; 40]);
                let v =
                    MomentData::from_fixed_point(gates, 2125, 250, 8, 2.0, 129.0, vec![vel; 40]);
                Radial::new(
                    ms + i as i64,
                    i as u16 + 1,
                    i as f32 * 45.0,
                    1.0,
                    if i == 0 {
                        RadialStatus::ScanStart
                    } else {
                        RadialStatus::IntermediateRadialData
                    },
                    number,
                    elevation,
                    Some(z),
                    Some(v),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .collect();
        Sweep::new(number, radials)
    }

    /// The lowest tilt passed at 60 s and again at 240 s (a SAILS revisit), the next tilt at
    /// 120 s; the two low passes read different velocities.
    fn sails_volume() -> Scan {
        let site = nexrad_model::meta::Site::new(*b"KTLX", 35.33, -97.28, 380, 0);
        Scan::with_site(
            site,
            vcp(),
            vec![
                sweep(1, 0.5, 60_000, 100),
                sweep(2, 0.9, 120_000, 140),
                sweep(1, 0.5, 240_000, 180),
            ],
        )
    }

    #[test]
    fn a_volume_lists_its_low_passes_in_time_order_from_its_start() {
        let scan = sails_volume();
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let all = passes(&scan, start);
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].sweep, all[1].sweep), (0, 2));
        assert!(all[0].time < all[1].time);
        // Passes before the volume's own time are not its own.
        let later = passes(&scan, DateTime::from_timestamp(100, 0).unwrap());
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].sweep, 2);
    }

    #[test]
    fn a_pass_reads_its_own_lowest_tilt_under_the_volumes_upper_tilts() {
        let scan = sails_volume();
        let start = DateTime::from_timestamp(0, 0).unwrap();
        // The volume's own pairs: the newest low pass, then the next tilt.
        let velocity: Vec<(BinnedSweep, BinnedSweep)> = (0..2)
            .map(|t| {
                (
                    level2::bin_scan_opts(&scan, Moment::Velocity, t, false).unwrap(),
                    level2::bin_scan(&scan, Moment::Reflectivity, t).unwrap(),
                )
            })
            .collect();
        let lowest = Lowest {
            velocity: true,
            ..Default::default()
        };
        let first = passes(&scan, start)[0];
        let at = at_pass(&scan, &first, &velocity, &[], &[], lowest).expect("binned");
        // Its lowest tilt is the earlier pass, binned as the pass is: not the newest.
        let own = level2::bin_sweep_index(&scan, Moment::Velocity, 0, true).unwrap();
        let newest = level2::bin_sweep_index(&scan, Moment::Velocity, 2, true).unwrap();
        assert_ne!(
            own.data, newest.data,
            "the two passes read different velocities"
        );
        assert_eq!(
            at.velocity[0].0.data, own.data,
            "the earlier pass's velocity"
        );
        assert_eq!(
            at.velocity[1].0.data, velocity[1].0.data,
            "the upper tilt unchanged"
        );
        // Without a lowest velocity tilt to replace, no pass inputs.
        assert!(at_pass(&scan, &first, &velocity, &[], &[], Lowest::default()).is_none());
    }

    /// A surveillance cut at `ms`: reflectivity code `z` and CC on every gate, no velocity.
    fn surveillance(number: u8, elevation: f32, ms: i64, z: u8) -> Sweep {
        let radials = (0..8)
            .map(|i| {
                let m = |code: u8| {
                    MomentData::from_fixed_point(40, 2125, 250, 8, 2.0, 66.0, vec![code; 40])
                };
                Radial::new(
                    ms + i as i64,
                    i as u16 + 1,
                    i as f32 * 45.0,
                    1.0,
                    if i == 0 {
                        RadialStatus::ScanStart
                    } else {
                        RadialStatus::IntermediateRadialData
                    },
                    number,
                    elevation,
                    Some(m(z)),
                    None,
                    None,
                    None,
                    None,
                    Some(m(200)),
                    None,
                )
            })
            .collect();
        Sweep::new(number, radials)
    }

    /// With `same_cut`, a pass's debris pair takes its reflectivity from the surveillance cut its
    /// CC came from; without, from the Doppler cut beside the velocity pass, as before (1008.md
    /// G1). The rotation inputs are the same either way.
    #[test]
    fn same_cut_pairs_a_pass_cc_with_its_own_reflectivity() {
        let site = nexrad_model::meta::Site::new(*b"KTLX", 35.33, -97.28, 380, 0);
        let scan = Scan::with_site(
            site,
            vcp(),
            vec![
                surveillance(1, 0.5, 40_000, 110),
                sweep(1, 0.5, 60_000, 100),
                sweep(2, 0.9, 120_000, 140),
                surveillance(1, 0.5, 220_000, 120),
                sweep(1, 0.5, 240_000, 180),
            ],
        );
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let first = passes(&scan, start)[0];
        let velocity = vec![(
            level2::bin_scan_opts(&scan, Moment::Velocity, 0, false).unwrap(),
            level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap(),
        )];
        let dual_pol = vec![(
            level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap(),
            level2::bin_scan(&scan, Moment::CorrelationCoefficient, 0).unwrap(),
        )];
        let lowest = Lowest {
            velocity: true,
            dual_pol: true,
            zdr: false,
        };
        let z = |i| {
            level2::bin_sweep_index(&scan, Moment::Reflectivity, i, false)
                .unwrap()
                .data
        };
        let before = at_pass_with(&scan, &first, &velocity, &dual_pol, &[], lowest, false).unwrap();
        let paired = at_pass_with(&scan, &first, &velocity, &dual_pol, &[], lowest, true).unwrap();
        assert_eq!(
            before.dual_pol[0].0.data,
            z(1),
            "the Doppler cut's reflectivity"
        );
        assert_eq!(
            paired.dual_pol[0].0.data,
            z(0),
            "the CC cut's own reflectivity"
        );
        assert_eq!(before.dual_pol[0].1.data, paired.dual_pol[0].1.data);
        assert_eq!(before.velocity[0].1.data, paired.velocity[0].1.data);
    }
}
