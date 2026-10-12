//! One storm seen by several radars, as one reflectivity volume (ROADMAP_NEW R1, 1010.md V18).
//! Research-grade.
//!
//! Each radar's volume is chosen by its own valid time, within a tolerance of the time asked
//! for, and its offset kept. Given the storm's motion ([`fuse_moving`]), each radar is read where
//! the storm was at its own volume's time, so a moving storm lines up across radars whose
//! volumes are a minute or two apart. The radars are then sampled on a common grid around the storm: x
//! east and y north in km, z in km above sea level. A radar contributes to a cell only through
//! a beam whose half-beamwidth covers the cell's height there, and then through the nearest such
//! beam's gate. Nothing is filled between beams, below the lowest or above the highest. A cell's
//! value is the weighted mean, in dBZ, of the radars that saw echo there, each weighted by
//! 1 / (its beam's thickness there)², so a nearby radar's thin beam counts for more than a
//! distant radar's wide one. A radar that saw below-threshold echo at a cell has observed it but
//! adds no echo. A cell no radar saw has no value.
//!
//! Every cell keeps its provenance: which radars observed it, which gave echo, and which counted
//! most. Radial velocities are never combined: each radar measures along its own beam, and an
//! average of them is not a wind.

use crate::level2::BinnedSweep;
use chrono::{DateTime, Utc};

/// The WSR-88D's half-power beamwidth, degrees.
pub const BEAMWIDTH_DEG: f64 = 0.95;

/// One radar's reflectivity volume, offered for fusion.
pub struct RadarVolume<'a> {
    pub site: String,
    pub time: DateTime<Utc>,
    /// Antenna altitude above sea level, metres.
    pub antenna_m: f64,
    /// The volume's reflectivity tilts.
    pub sweeps: &'a [BinnedSweep],
}

/// The grid to fuse onto: a box `2 half_km` wide around `center` (`[lon, lat]`), `n` by `n`
/// columns, `nz` levels from `bottom_km` to `top_km` above sea level.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub center: [f64; 2],
    pub half_km: f64,
    pub n: usize,
    pub nz: usize,
    pub bottom_km: f64,
    pub top_km: f64,
}

impl Grid {
    /// Cell `(i, j, k)`'s offset from the centre, km east and north, and height, km above sea
    /// level.
    pub fn cell(&self, i: usize, j: usize, k: usize) -> (f64, f64, f64) {
        let step = |m: usize, count: usize, lo: f64, hi: f64| {
            if count < 2 {
                (lo + hi) / 2.0
            } else {
                lo + (hi - lo) * m as f64 / (count - 1) as f64
            }
        };
        (
            step(i, self.n, -self.half_km, self.half_km),
            step(j, self.n, -self.half_km, self.half_km),
            step(k, self.nz, self.bottom_km, self.top_km),
        )
    }

    /// The `[lon, lat]` of a point `x` km east and `y` km north of the centre.
    pub fn lonlat(&self, x: f64, y: f64) -> [f64; 2] {
        let km_per_deg = 6371.0f64.to_radians();
        let coslat = self.center[1].to_radians().cos().max(0.05);
        [
            self.center[0] + x / (km_per_deg * coslat),
            self.center[1] + y / km_per_deg,
        ]
    }

    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n * j + self.n * self.n * k
    }

    pub fn len(&self) -> usize {
        self.n * self.n * self.nz
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A radar left out of a fusion, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct LeftOut {
    pub site: String,
    pub reason: String,
}

/// The fused volume.
#[derive(Debug, Clone)]
pub struct FusedVolume {
    pub grid: Grid,
    pub time: DateTime<Utc>,
    /// The radars used, in the order of the bits below.
    pub sites: Vec<String>,
    /// Each used radar's volume time less `time`, seconds.
    pub offsets_s: Vec<i64>,
    pub left_out: Vec<LeftOut>,
    /// The storm motion each radar was read along, m/s east and north, if one was given.
    pub motion_ms: Option<[f64; 2]>,
    /// Per cell (see [`Grid::index`]): the fused dBZ, NaN where no radar saw echo.
    pub dbz: Vec<f32>,
    /// Per cell, a bit per used radar that observed it (echo or below threshold).
    pub observed: Vec<u32>,
    /// Per cell, a bit per used radar that gave echo.
    pub echo: Vec<u32>,
    /// Per cell, the radar that counted most (its index in `sites`), or `u8::MAX`.
    pub dominant: Vec<u8>,
}

/// What one radar saw at one cell.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    /// No beam covers the cell.
    Unseen,
    /// Below threshold through a beam this thick (km).
    Clear,
    /// Echo of this dBZ through a beam this thick (km).
    Echo(f32, f64),
}

/// What `sweeps` (one radar) saw at a point `ground_km` from it at azimuth `az` and `z_km` above
/// its antenna.
fn look(sweeps: &[BinnedSweep], ground_km: f64, az: f64, z_km: f64) -> Look {
    let half_beam = (BEAMWIDTH_DEG / 2.0).to_radians();
    let mut best: Option<(f64, Look)> = None;
    for s in sweeps {
        let Some((cell, slant)) = crate::xsection::gate_over_ground(s, ground_km, az) else {
            continue;
        };
        let e = f64::from(s.elevation_deg);
        let h = crate::xsection::beam_height_km(slant, e);
        let half = slant * half_beam.tan();
        let miss = (h - z_km).abs();
        if miss > half {
            continue;
        }
        let idx = s.data[cell];
        let seen = if idx >= 2 {
            Look::Echo(
                s.value_min + (idx as f32 - 2.0) / 253.0 * (s.value_max - s.value_min),
                2.0 * half,
            )
        } else {
            Look::Clear
        };
        // The beam whose centre is nearest the cell.
        if best.is_none_or(|(m, _)| miss < m) {
            best = Some((miss, seen));
        }
    }
    best.map_or(Look::Unseen, |(_, l)| l)
}

/// Fuse `radars` onto `grid` for `time`: each radar whose volume is within `max_offset_s` of
/// it, at most 32. The others are left out and said so. Each radar is read where the grid's
/// cells are, whatever its volume's offset; [`fuse_moving`] follows the storm instead.
pub fn fuse(
    radars: &[RadarVolume<'_>],
    time: DateTime<Utc>,
    max_offset_s: i64,
    grid: Grid,
) -> FusedVolume {
    fuse_inner(radars, time, max_offset_s, grid, None)
}

/// [`fuse`], with each radar read where the storm was at its own volume's time: a cell's place
/// moved by `motion_ms` (m/s east and north) times that radar's offset.
pub fn fuse_moving(
    radars: &[RadarVolume<'_>],
    time: DateTime<Utc>,
    max_offset_s: i64,
    grid: Grid,
    motion_ms: [f64; 2],
) -> FusedVolume {
    fuse_inner(radars, time, max_offset_s, grid, Some(motion_ms))
}

/// The radars within `max_offset_s` of `time` (at most 32), and the others with why.
fn select<'r, 'a>(
    radars: &'r [RadarVolume<'a>],
    time: DateTime<Utc>,
    max_offset_s: i64,
) -> (Vec<&'r RadarVolume<'a>>, Vec<LeftOut>) {
    let mut used: Vec<&RadarVolume<'_>> = Vec::new();
    let mut left_out = Vec::new();
    for r in radars {
        let offset = (r.time - time).num_seconds();
        if offset.abs() > max_offset_s {
            left_out.push(LeftOut {
                site: r.site.clone(),
                reason: format!("its volume is {offset} s from the time asked for"),
            });
        } else if r.sweeps.is_empty() {
            left_out.push(LeftOut {
                site: r.site.clone(),
                reason: "no reflectivity tilts".into(),
            });
        } else if used.len() == 32 {
            left_out.push(LeftOut {
                site: r.site.clone(),
                reason: "more than 32 radars".into(),
            });
        } else {
            used.push(r);
        }
    }
    (used, left_out)
}

/// Each used radar's ground range and azimuth (km, degrees) to the column at `ll`, where the
/// storm put it at that radar's time when `motion_ms` is given.
fn column_geometry(
    used: &[&RadarVolume<'_>],
    time: DateTime<Utc>,
    motion_ms: Option<[f64; 2]>,
    ll: [f64; 2],
) -> Vec<(f64, f64)> {
    let km_per_deg = 6371.0f64.to_radians();
    let coslat = ll[1].to_radians().cos().max(0.05);
    used.iter()
        .map(|r| {
            let dt = (r.time - time).num_milliseconds() as f64 / 1000.0;
            let [ue, vn] = motion_ms.unwrap_or([0.0, 0.0]);
            let at = [
                ll[0] + ue * dt / 1000.0 / (km_per_deg * coslat),
                ll[1] + vn * dt / 1000.0 / km_per_deg,
            ];
            let s = &r.sweeps[0];
            crate::xsection::dist_bearing(
                f64::from(s.radar_lon),
                f64::from(s.radar_lat),
                at[0],
                at[1],
            )
        })
        .collect()
}

/// One cell `z_km` above sea level over a column: the fused dBZ (NaN without echo), the radars
/// that observed it and gave echo (bits), and the one that counted most (or `u8::MAX`).
fn fuse_cell(used: &[&RadarVolume<'_>], geometry: &[(f64, f64)], z_km: f64) -> (f32, u32, u32, u8) {
    let (mut sum, mut wsum, mut top) = (0.0f64, 0.0f64, (0.0f64, u8::MAX));
    let (mut observed, mut echo) = (0u32, 0u32);
    for (r_i, (r, &(ground, az))) in used.iter().zip(geometry).enumerate() {
        match look(r.sweeps, ground, az, z_km - r.antenna_m / 1000.0) {
            Look::Unseen => {}
            Look::Clear => observed |= 1 << r_i,
            Look::Echo(v, thick) => {
                observed |= 1 << r_i;
                echo |= 1 << r_i;
                let w = 1.0 / (thick * thick).max(1e-6);
                sum += w * f64::from(v);
                wsum += w;
                if w > top.0 {
                    top = (w, r_i as u8);
                }
            }
        }
    }
    if wsum > 0.0 {
        ((sum / wsum) as f32, observed, echo, top.1)
    } else {
        (f32::NAN, observed, echo, u8::MAX)
    }
}

fn fuse_inner(
    radars: &[RadarVolume<'_>],
    time: DateTime<Utc>,
    max_offset_s: i64,
    grid: Grid,
    motion_ms: Option<[f64; 2]>,
) -> FusedVolume {
    let (used, left_out) = select(radars, time, max_offset_s);
    let len = grid.len();
    let mut out = FusedVolume {
        grid,
        time,
        sites: used.iter().map(|r| r.site.clone()).collect(),
        offsets_s: used.iter().map(|r| (r.time - time).num_seconds()).collect(),
        left_out,
        motion_ms,
        dbz: vec![f32::NAN; len],
        observed: vec![0; len],
        echo: vec![0; len],
        dominant: vec![u8::MAX; len],
    };
    for j in 0..grid.n {
        for i in 0..grid.n {
            let (x, y, _) = grid.cell(i, j, 0);
            let geometry = column_geometry(&used, time, motion_ms, grid.lonlat(x, y));
            for k in 0..grid.nz {
                let (_, _, z) = grid.cell(i, j, k);
                let at = grid.index(i, j, k);
                let (v, o, e, d) = fuse_cell(&used, &geometry, z);
                out.dbz[at] = v;
                out.observed[at] = o;
                out.echo[at] = e;
                out.dominant[at] = d;
            }
        }
    }
    out
}

/// A vertical section fused from several radars, along the straight lon/lat line `a` to `b`, as
/// the single-radar [`crate::xsection::build`] draws one: `cols` columns, `rows` rows from
/// `max_height_km` (row 0) down to the ground, heights above the first radar's antenna so the
/// two read on one axis. `beam_covered` is true where a radar saw echo; there are no beam-rise
/// lines, since several radars' beams cross the panel. With the section, which radars went in
/// (the bits' order), their offsets, those left out, and per cell the radar that counted most.
pub struct FusedSection {
    pub section: crate::xsection::CrossSection,
    pub sites: Vec<String>,
    pub offsets_s: Vec<i64>,
    pub left_out: Vec<LeftOut>,
    pub dominant: Vec<u8>,
}

#[allow(clippy::too_many_arguments)] // the line, its raster and the time base, all distinct
pub fn fuse_section(
    radars: &[RadarVolume<'_>],
    time: DateTime<Utc>,
    max_offset_s: i64,
    a: (f64, f64),
    b: (f64, f64),
    cols: usize,
    rows: usize,
    max_height_km: f32,
) -> Option<FusedSection> {
    let (used, left_out) = select(radars, time, max_offset_s);
    let base_km = used.first()?.antenna_m / 1000.0;
    let (cols, rows) = (cols.max(2), rows.max(2));
    let length_km = crate::xsection::dist_bearing(a.0, a.1, b.0, b.1).0;
    let mut dbz = vec![None; cols * rows];
    let mut beam_covered = vec![false; cols * rows];
    let mut dominant = vec![u8::MAX; cols * rows];
    for i in 0..cols {
        let t = i as f64 / (cols - 1) as f64;
        let ll = [a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t];
        let geometry = column_geometry(&used, time, None, ll);
        for r in 0..rows {
            let h = f64::from(max_height_km) * (1.0 - r as f64 / (rows - 1) as f64);
            let (v, _, e, d) = fuse_cell(&used, &geometry, h + base_km);
            let at = r * cols + i;
            if !v.is_nan() {
                dbz[at] = Some(v);
            }
            beam_covered[at] = e != 0;
            dominant[at] = d;
        }
    }
    Some(FusedSection {
        section: crate::xsection::CrossSection {
            cols,
            rows,
            max_height_km,
            length_km,
            dbz,
            beam_covered,
            beam_lines: Vec::new(),
        },
        sites: used.iter().map(|r| r.site.clone()).collect(),
        offsets_s: used.iter().map(|r| (r.time - time).num_seconds()).collect(),
        left_out,
        dominant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// A reflectivity volume of a radar at `radar`, on VCP 12-like tilts, from `dbz(point,
    /// height km above sea level)`, gates out to `max_km`.
    fn volume(
        radar: [f64; 2],
        antenna_m: f64,
        max_km: f32,
        dbz: impl Fn([f64; 2], f64) -> f32,
    ) -> Vec<BinnedSweep> {
        const TILTS: [f32; 14] = [
            0.5, 0.9, 1.3, 1.8, 2.4, 3.1, 4.0, 5.1, 6.4, 8.0, 10.0, 12.5, 15.6, 19.5,
        ];
        let (bins, dg) = (360usize, 1.0f32);
        let gates = (max_km / dg) as usize;
        let encode = |z: f32| ((z + 32.0) / 127.0 * 253.0 + 2.0).clamp(2.0, 255.0) as u8;
        TILTS
            .iter()
            .map(|&e| {
                let mut data = vec![1u8; bins * gates];
                for b in 0..bins {
                    let az = (b as f64 + 0.5) * 360.0 / bins as f64;
                    for g in 0..gates {
                        let slant = g as f64 * f64::from(dg);
                        let ground = crate::xsection::ground_from_slant_km(slant, f64::from(e));
                        let q = crate::rotation::dest(radar[0], radar[1], az, ground);
                        let h = crate::xsection::beam_height_km(slant, f64::from(e))
                            + antenna_m / 1000.0;
                        let z = dbz([q.0, q.1], h);
                        if z > 0.0 {
                            data[b * gates + g] = encode(z);
                        }
                    }
                }
                BinnedSweep {
                    moment: crate::level2::Moment::Reflectivity,
                    az_bins: bins,
                    gate_count: gates,
                    data,
                    first_gate_km: 0.0,
                    gate_interval_km: dg,
                    radar_lat: radar[1] as f32,
                    radar_lon: radar[0] as f32,
                    elevation_deg: e,
                    value_min: -32.0,
                    value_max: 95.0,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn km(a: [f64; 2], b: [f64; 2]) -> f64 {
        crate::xsection::dist_bearing(a[0], a[1], b[0], b[1]).0
    }

    /// A storm 50 dBZ at its core, falling 2 dB a km from its axis, from the ground to 14 km.
    fn storm(c: [f64; 2]) -> impl Fn([f64; 2], f64) -> f32 + Copy {
        move |q, h| {
            if h > 14.0 {
                return 0.0;
            }
            (50.0 - 2.0 * km(c, q) as f32).max(0.0)
        }
    }

    /// A radar 15 km from the storm cannot see its upper half (its highest tilt passes about
    /// 5 km up there); one 110 km away sees the top through wide beams but not the lowest 1.5 km.
    /// Fused, the column is filled from the ground to the top: the near radar below, the far one
    /// above, each cell said to be from the radar that saw it, and the values the storm's.
    #[test]
    fn two_radars_fill_each_others_gaps_and_say_which_saw_what() {
        let c = [-97.5, 35.3];
        let near = crate::rotation::dest(c[0], c[1], 270.0, 15.0);
        let far = crate::rotation::dest(c[0], c[1], 90.0, 110.0);
        let t = Utc.with_ymd_and_hms(2013, 5, 20, 20, 10, 0).unwrap();
        let a = volume([near.0, near.1], 0.0, 40.0, storm(c));
        let b = volume([far.0, far.1], 0.0, 140.0, storm(c));
        let radars = [
            RadarVolume {
                site: "NEAR".into(),
                time: t,
                antenna_m: 0.0,
                sweeps: &a,
            },
            RadarVolume {
                site: "FAR".into(),
                time: t + chrono::Duration::seconds(120),
                antenna_m: 0.0,
                sweeps: &b,
            },
        ];
        let grid = Grid {
            center: c,
            half_km: 4.0,
            n: 9,
            nz: 27,
            bottom_km: 0.5,
            top_km: 13.5,
        };
        let f = fuse(&radars, t, 300, grid);
        assert_eq!(f.sites, ["NEAR", "FAR"]);
        assert_eq!(f.offsets_s, [0, 120]);
        let mid = grid.n / 2;
        let (mut near_only, mut far_only, mut both) = (0, 0, 0);
        for k in 0..grid.nz {
            let (_, _, z) = grid.cell(mid, mid, k);
            let at = grid.index(mid, mid, k);
            match f.echo[at] {
                0b01 => near_only += 1,
                0b10 => far_only += 1,
                0b11 => both += 1,
                _ => {}
            }
            // The storm's axis is 50 dBZ all the way up; the fused value is close to it
            // wherever any radar saw it.
            if !f.dbz[at].is_nan() {
                assert!((f.dbz[at] - 50.0).abs() < 4.0, "{z} km: {}", f.dbz[at]);
            }
            // Nothing above the storm's top.
            if z > 14.5 {
                assert!(f.dbz[at].is_nan());
            }
        }
        assert!(near_only > 0, "the near radar alone low down");
        assert!(far_only > 0, "the far radar alone up high");
        // Where both saw it, the near radar's thin beam counts for more.
        for k in 0..grid.nz {
            let at = grid.index(mid, mid, k);
            if f.echo[at] == 0b11 {
                assert_eq!(f.dominant[at], 0, "level {k}");
            }
        }
        let filled = (0..grid.nz)
            .filter(|&k| !f.dbz[grid.index(mid, mid, k)].is_nan())
            .count();
        let near_alone = (0..grid.nz)
            .filter(|&k| f.echo[grid.index(mid, mid, k)] & 1 != 0)
            .count();
        assert!(
            filled > near_alone,
            "{filled} levels fused, {near_alone} from the near radar"
        );
        let _ = both;
    }

    /// The Moore storm at 20:12 UTC on 2013-05-20, seen by KTLX about 22 km away and by KFDR,
    /// KINX and KVNX 140 to 170 km away (the archive, cached under `target/tmp/feature-track`).
    /// KTLX's highest tilt passes only 7 to 8 km over the storm; fused, the distant radars fill
    /// the storm's upper half. Prints what each radar gave, and the echo top with and without
    /// them. Writes `target/parity-review/v18/moore-fusion.txt`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "network: four radars' archive volumes"]
    async fn live_the_moore_storm_fused_from_four_radars() {
        use crate::level2::{self, Moment};
        let at = Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 0).unwrap();
        let cache =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/feature-track");
        std::fs::create_dir_all(&cache).unwrap();
        let mut held = Vec::new();
        for site in ["KTLX", "KFDR", "KINX", "KVNX"] {
            let ids = level2::list_volumes(site, at.date_naive()).await.unwrap();
            let id = ids
                .into_iter()
                .filter(|id| id.date_time().is_some())
                .min_by_key(|id| (id.date_time().unwrap() - at).num_seconds().abs())
                .unwrap();
            let time = id.date_time().unwrap();
            let scan = level2::download_scan(id, Some(cache.clone()))
                .await
                .unwrap();
            let sweeps: Vec<BinnedSweep> = (0..level2::elevation_angles(&scan).len())
                .filter_map(|t| {
                    level2::bin_scan_opts_recorded(&scan, Moment::Reflectivity, t, false).ok()
                })
                .collect();
            let antenna_m =
                crate::sites::site_by_id(site).map_or(0.0, |s| f64::from(s.elevation_meters));
            held.push((site.to_string(), time, antenna_m, sweeps));
        }
        let radars: Vec<RadarVolume<'_>> = held
            .iter()
            .map(|(site, time, antenna_m, sweeps)| RadarVolume {
                site: site.clone(),
                time: *time,
                antenna_m: *antenna_m,
                sweeps,
            })
            .collect();
        // 40 km across around Moore, 1 km columns, 0.5 km levels from 0.5 to 16.5 km.
        let grid = Grid {
            center: [-97.50, 35.33],
            half_km: 20.0,
            n: 41,
            nz: 33,
            bottom_km: 0.5,
            top_km: 16.5,
        };
        let all = fuse(&radars, at, 300, grid);
        let alone = fuse(&radars[..1], at, 300, grid);
        // The highest level with at least 18.5 dBZ anywhere in the box, and how many cells had echo.
        let summary = |f: &FusedVolume| {
            let mut top = f64::NAN;
            let mut cells = 0usize;
            for k in 0..grid.nz {
                for j in 0..grid.n {
                    for i in 0..grid.n {
                        let v = f.dbz[grid.index(i, j, k)];
                        if !v.is_nan() {
                            cells += 1;
                            if v >= 18.5 {
                                top = grid.cell(i, j, k).2;
                            }
                        }
                    }
                }
            }
            (top, cells)
        };
        let (top_all, cells_all) = summary(&all);
        let (top_alone, cells_alone) = summary(&alone);
        let mut report = String::from(
            "Moore, 2013-05-20 20:12 UTC; 40 km box around -97.50, 35.33; 1 km columns, 0.5 km \
             levels to 16.5 km\n",
        );
        for (i, site) in all.sites.iter().enumerate() {
            let echo_cells = all.echo.iter().filter(|&&e| e & (1 << i) != 0).count();
            let only = all.echo.iter().filter(|&&e| e == 1 << i).count();
            let leads = all
                .dominant
                .iter()
                .filter(|&&d| usize::from(d) == i)
                .count();
            report.push_str(&format!(
                "{site}: volume {:+} s; echo in {echo_cells} cells, {only} of them from it alone; counted most in {leads}\n",
                all.offsets_s[i]
            ));
        }
        for l in &all.left_out {
            report.push_str(&format!("{} left out: {}\n", l.site, l.reason));
        }
        report.push_str(&format!(
            "KTLX alone: {cells_alone} cells with echo, 18.5 dBZ top {top_alone:.1} km\nfused: {cells_all} cells with echo, 18.5 dBZ top {top_all:.1} km\n"
        ));
        // How well each distant radar agrees with KTLX where both saw echo, read in place and
        // along the storm's motion (8.9 m/s toward 73 degrees, the followed mesocyclone's).
        let motion = [
            8.9 * 73f64.to_radians().sin(),
            8.9 * 73f64.to_radians().cos(),
        ];
        let agreement = |moving: bool| -> Vec<(String, usize, f64, f64)> {
            let one = |r: &[RadarVolume<'_>]| {
                if moving {
                    fuse_moving(r, at, 300, grid, motion)
                } else {
                    fuse(r, at, 300, grid)
                }
            };
            let ktlx = one(&radars[..1]);
            radars[1..]
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let other = one(&radars[1 + i..2 + i]);
                    let (mut n, mut sum, mut signed) = (0usize, 0.0f64, 0.0f64);
                    for (a, b) in ktlx.dbz.iter().zip(&other.dbz) {
                        if !a.is_nan() && !b.is_nan() {
                            n += 1;
                            sum += f64::from((a - b).abs());
                            signed += f64::from(a - b);
                        }
                    }
                    let n_f = n.max(1) as f64;
                    (r.site.clone(), n, sum / n_f, signed / n_f)
                })
                .collect()
        };
        let (still, moving) = (agreement(false), agreement(true));
        for ((site, n, d0, b0), (_, n1, d1, _)) in still.iter().zip(&moving) {
            report.push_str(&format!(
                "KTLX against {site}, where both saw echo: mean difference {d0:.2} dB over {n} cells \
                 read in place (KTLX minus it, on average: {b0:+.2} dB), {d1:.2} dB over {n1} read \
                 along the storm's motion\n"
            ));
        }
        print!("{report}");
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/parity-review/v18");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("moore-fusion.txt"), &report).unwrap();
        assert_eq!(all.sites.len(), 4, "{:?}", all.left_out);
        assert!(cells_all > cells_alone);
        assert!(top_all > top_alone, "{top_all} vs {top_alone}");
    }

    /// A storm moving 20 m/s east, seen by the far radar 120 s after the near one: read where the
    /// cells are, the far radar's view of the storm's axis is 2.4 km off it and reads low; read
    /// along the storm's motion, it is on the axis again.
    #[test]
    fn a_moving_storm_lines_up_when_each_radar_is_read_where_it_was() {
        let c = [-97.5, 35.3];
        let near = crate::rotation::dest(c[0], c[1], 270.0, 15.0);
        let far = crate::rotation::dest(c[0], c[1], 90.0, 110.0);
        let t = Utc.with_ymd_and_hms(2013, 5, 20, 20, 10, 0).unwrap();
        // 20 m/s for 120 s is 2.4 km east.
        let moved = crate::rotation::dest(c[0], c[1], 90.0, 2.4);
        let a = volume([near.0, near.1], 0.0, 40.0, storm(c));
        let b = volume([far.0, far.1], 0.0, 140.0, storm([moved.0, moved.1]));
        let radars = [
            RadarVolume {
                site: "NEAR".into(),
                time: t,
                antenna_m: 0.0,
                sweeps: &a,
            },
            RadarVolume {
                site: "FAR".into(),
                time: t + chrono::Duration::seconds(120),
                antenna_m: 0.0,
                sweeps: &b,
            },
        ];
        let grid = Grid {
            center: c,
            half_km: 0.0,
            n: 1,
            nz: 27,
            bottom_km: 0.5,
            top_km: 13.5,
        };
        // The levels only the far radar saw, on the axis.
        let far_only = |f: &FusedVolume| -> Vec<f32> {
            (0..grid.nz)
                .map(|k| grid.index(0, 0, k))
                .filter(|&at| f.echo[at] == 0b10)
                .map(|at| f.dbz[at])
                .collect()
        };
        let still = far_only(&fuse(&radars, t, 300, grid));
        let moving = far_only(&fuse_moving(&radars, t, 300, grid, [20.0, 0.0]));
        assert!(!still.is_empty() && !moving.is_empty());
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean(&still) < 47.0, "read in place: {still:?}");
        assert!(
            (mean(&moving) - 50.0).abs() < 2.5,
            "read along the motion: {moving:?}"
        );
    }

    /// A fused section along a line reads what the fused volume reads at the same places, and
    /// says which radars went in.
    #[test]
    fn a_fused_section_matches_the_volume_along_its_line() {
        let c = [-97.5, 35.3];
        let near = crate::rotation::dest(c[0], c[1], 270.0, 15.0);
        let far = crate::rotation::dest(c[0], c[1], 90.0, 110.0);
        let t = Utc.with_ymd_and_hms(2013, 5, 20, 20, 10, 0).unwrap();
        let a = volume([near.0, near.1], 0.0, 40.0, storm(c));
        let b = volume([far.0, far.1], 0.0, 140.0, storm(c));
        let radars = [
            RadarVolume {
                site: "NEAR".into(),
                time: t,
                antenna_m: 0.0,
                sweeps: &a,
            },
            RadarVolume {
                site: "FAR".into(),
                time: t,
                antenna_m: 0.0,
                sweeps: &b,
            },
        ];
        let w = crate::rotation::dest(c[0], c[1], 270.0, 4.0);
        let e = crate::rotation::dest(c[0], c[1], 90.0, 4.0);
        let s = fuse_section(&radars, t, 300, w, e, 9, 33, 16.0).unwrap();
        assert_eq!(s.sites, ["NEAR", "FAR"]);
        assert_eq!((s.section.cols, s.section.rows), (9, 33));
        // The middle column, every height, against the volume's centre column.
        let grid = Grid {
            center: c,
            half_km: 0.0,
            n: 1,
            nz: 33,
            bottom_km: 0.0,
            top_km: 16.0,
        };
        let f = fuse(&radars, t, 300, grid);
        for r in 0..33 {
            let k = 32 - r;
            let vol = f.dbz[grid.index(0, 0, k)];
            let sec = s.section.dbz[r * 9 + 4];
            match sec {
                Some(v) => assert!((v - vol).abs() < 0.5, "row {r}: {v} vs {vol}"),
                None => assert!(vol.is_nan(), "row {r}"),
            }
        }
        // Echo somewhere up the storm's axis, and none above its top.
        assert!(s.section.dbz.iter().flatten().any(|v| *v > 45.0));
        assert!((0..9).all(|i| s.section.dbz[i].is_none()));
    }

    /// A radar whose volume is too far from the time asked for is left out, and said so; no
    /// radars gives an empty volume, not a made-up one.
    #[test]
    fn a_radar_out_of_time_is_left_out_and_nothing_is_invented() {
        let c = [-97.5, 35.3];
        let t = Utc.with_ymd_and_hms(2013, 5, 20, 20, 10, 0).unwrap();
        let near = crate::rotation::dest(c[0], c[1], 270.0, 15.0);
        let a = volume([near.0, near.1], 0.0, 40.0, storm(c));
        let grid = Grid {
            center: c,
            half_km: 2.0,
            n: 3,
            nz: 4,
            bottom_km: 1.0,
            top_km: 4.0,
        };
        let late = [RadarVolume {
            site: "LATE".into(),
            time: t + chrono::Duration::seconds(600),
            antenna_m: 0.0,
            sweeps: &a,
        }];
        let f = fuse(&late, t, 300, grid);
        assert!(f.sites.is_empty());
        assert_eq!(f.left_out.len(), 1);
        assert!(f.left_out[0].reason.contains("600 s"), "{:?}", f.left_out);
        assert!(f.dbz.iter().all(|v| v.is_nan()));
        assert!(f.observed.iter().all(|&o| o == 0));
    }
}
