//! Hydrometeor size sorting beside a rotation column (detectionplan.md, "Dual-pol precursors"):
//! the ZDR arc and the KDP foot at the lowest tilt, how far apart they lie, and at what angle to
//! the storm's motion.
//!
//! Low-level storm-relative helicity sorts a supercell's drops by size: the large, sparse drops
//! that read as high differential reflectivity (the ZDR arc, along the forward flank's inflow
//! edge) fall out apart from the many smaller drops that make the specific differential phase
//! (the KDP foot). Loeffler, Kumjian et al. (2020, *Geophys. Res. Lett.* 47, e2020GL088242) found
//! the two further apart, and the line between them more nearly across the storm's motion, in
//! supercells about to produce tornadoes than in those that would not: evidence that comes
//! before touchdown, where debris can only follow it.
//!
//! Measured on a regular grid around the column (every [`STEP_KM`] within [`RADIUS_KM`]), each
//! moment read where the radar recorded it (`BinnedSweep::sample_at`), so the moments need not
//! share a gate layout.

use crate::level2::BinnedSweep;

/// The arc: differential reflectivity at least this (dB), in rain (`Z`, `CC` below).
pub const ZDR_ARC_DB: f32 = 3.25;
/// The foot: specific differential phase at least this (degrees per km).
pub const KDP_FOOT_DEG_KM: f32 = 1.5;
/// Rain, not noise or debris: reflectivity at least this (dBZ) and CC at least [`MIN_CC`].
pub const MIN_Z_DBZ: f32 = 20.0;
pub const MIN_CC: f32 = 0.9;
/// How far from the column the arc and foot are looked for (km), and the grid's spacing.
pub const RADIUS_KM: f64 = 20.0;
pub const STEP_KM: f64 = 1.0;
/// Fewest grid cells an arc or a foot must cover to count (here 3 km²).
pub const MIN_CELLS: usize = 3;

/// The arc and the foot beside one column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeSorting {
    /// From the foot's centroid to the arc's (km).
    pub separation_km: f32,
    /// Between that line and the storm's motion, folded into 0-90 degrees (90: across it);
    /// `None` with no motion.
    pub angle_deg: Option<f32>,
    pub arc_cells: usize,
    pub foot_cells: usize,
}

/// One grid point's readings: reflectivity, ZDR, KDP and CC, each `None` where nothing was
/// recorded.
pub type Readings = (Option<f32>, Option<f32>, Option<f32>, Option<f32>);

/// The arc and foot within [`RADIUS_KM`] of `(lon, lat)` on the lowest tilt's sweeps, with the
/// storm moving `motion` (m/s east, north). `None` when either covers fewer than [`MIN_CELLS`].
pub fn measure(
    z: &BinnedSweep,
    zdr: &BinnedSweep,
    kdp: &BinnedSweep,
    cc: &BinnedSweep,
    lon: f64,
    lat: f64,
    motion: Option<(f32, f32)>,
) -> Option<SizeSorting> {
    let read = |x: f64, y: f64| {
        let v = |s: &BinnedSweep| s.sample_at(x, y).and_then(|g| g.value);
        (v(z), v(zdr), v(kdp), v(cc))
    };
    measure_with(read, lon, lat, motion)
}

/// [`measure`] with the readings at any `(lon, lat)` from `read`.
pub fn measure_with(
    read: impl Fn(f64, f64) -> Readings,
    lon: f64,
    lat: f64,
    motion: Option<(f32, f32)>,
) -> Option<SizeSorting> {
    let km_lon = 111.32 * lat.to_radians().cos();
    let km_lat = 110.57;
    let n = (RADIUS_KM / STEP_KM) as i64;
    let (mut arc, mut foot) = ((0.0, 0.0, 0usize), (0.0, 0.0, 0usize));
    for i in -n..=n {
        for j in -n..=n {
            let (x, y) = (i as f64 * STEP_KM, j as f64 * STEP_KM);
            if x.hypot(y) > RADIUS_KM {
                continue;
            }
            let (rz, rzdr, rkdp, rcc) = read(lon + x / km_lon, lat + y / km_lat);
            let rain = rz.is_some_and(|v| v >= MIN_Z_DBZ) && rcc.is_some_and(|v| v >= MIN_CC);
            if !rain {
                continue;
            }
            if rzdr.is_some_and(|v| v >= ZDR_ARC_DB) {
                arc = (arc.0 + x, arc.1 + y, arc.2 + 1);
            }
            if rkdp.is_some_and(|v| v >= KDP_FOOT_DEG_KM) {
                foot = (foot.0 + x, foot.1 + y, foot.2 + 1);
            }
        }
    }
    if arc.2 < MIN_CELLS || foot.2 < MIN_CELLS {
        return None;
    }
    let (ax, ay) = (arc.0 / arc.2 as f64, arc.1 / arc.2 as f64);
    let (fx, fy) = (foot.0 / foot.2 as f64, foot.1 / foot.2 as f64);
    let (dx, dy) = (ax - fx, ay - fy);
    let separation = dx.hypot(dy);
    let angle = motion.and_then(|(u, v)| {
        let speed = (u as f64).hypot(v as f64);
        (speed > 0.5 && separation > 0.0).then(|| {
            let cos = ((dx * u as f64 + dy * v as f64) / (separation * speed)).abs();
            cos.clamp(0.0, 1.0).acos().to_degrees() as f32
        })
    });
    Some(SizeSorting {
        separation_km: separation as f32,
        angle_deg: angle,
        arc_cells: arc.2,
        foot_cells: foot.2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LON: f64 = -97.5;
    const LAT: f64 = 35.5;

    /// Rain everywhere within 20 km, with an arc of high ZDR around `arc` and a foot of high KDP
    /// around `foot` (km east, north of the column), each 2 km across.
    fn storm(arc: (f64, f64), foot: (f64, f64)) -> impl Fn(f64, f64) -> Readings {
        move |lon, lat| {
            let x = (lon - LON) * 111.32 * LAT.to_radians().cos();
            let y = (lat - LAT) * 110.57;
            let near = |c: (f64, f64)| (x - c.0).hypot(y - c.1) <= 2.0;
            (
                Some(45.0),
                Some(if near(arc) { 4.0 } else { 1.0 }),
                Some(if near(foot) { 2.5 } else { 0.3 }),
                Some(0.98),
            )
        }
    }

    #[test]
    fn the_arc_and_foot_are_found_where_they_are() {
        // Storm moving east; the arc 6 km north of the foot: across the motion.
        let s = measure_with(storm((2.0, 6.0), (2.0, 0.0)), LON, LAT, Some((15.0, 0.0))).unwrap();
        assert!((s.separation_km - 6.0).abs() < 0.3, "{s:?}");
        assert!(s.angle_deg.unwrap() > 85.0, "{s:?}");
        // The same pair along the motion.
        let along =
            measure_with(storm((8.0, 0.0), (2.0, 0.0)), LON, LAT, Some((15.0, 0.0))).unwrap();
        assert!(along.angle_deg.unwrap() < 5.0, "{along:?}");
        // No motion: no angle.
        assert!(measure_with(storm((2.0, 6.0), (2.0, 0.0)), LON, LAT, None)
            .unwrap()
            .angle_deg
            .is_none());
    }

    #[test]
    fn no_arc_debris_or_dry_air_reads_as_nothing() {
        // No arc at all: the foot alone is no measurement.
        let no_arc = |lon: f64, lat: f64| {
            let (z, _, kdp, cc) = storm((2.0, 6.0), (2.0, 0.0))(lon, lat);
            (z, Some(1.0), kdp, cc)
        };
        assert!(measure_with(no_arc, LON, LAT, None).is_none());
        // Low CC (debris, non-meteorological echo) is not rain.
        let debris = |lon: f64, lat: f64| {
            let (z, zdr, kdp, _) = storm((2.0, 6.0), (2.0, 0.0))(lon, lat);
            (z, zdr, kdp, Some(0.6))
        };
        assert!(measure_with(debris, LON, LAT, None).is_none());
        // Nothing recorded.
        assert!(measure_with(|_, _| (None, None, None, None), LON, LAT, None).is_none());
    }
}
