//! Every low-level pass of a volume (detectionplan.md, "Every low-level pass"). Under SAILS or
//! MRLE the radar revisits its lowest tilt partway through the volume: 68% of the backtest's
//! volumes have two to four velocity passes there, a median 1.8 minutes apart in volumes 5.7
//! minutes long. The Tornado ID's columns are built and tracked at each of them, each pass at the
//! lowest tilt under the volume's own upper tilts, so a tornado is marked at the pass that first
//! shows it rather than at the volume's last. On four random tornado samples that found 6-10
//! points more of the tornadoes and moved the first marker's median lead from about +1 to +3
//! minutes.
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
        match nearest(Moment::CorrelationCoefficient) {
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
