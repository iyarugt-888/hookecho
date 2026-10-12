//! `hookecho --headless-fusion SITES TIME LON,LAT <out.png>` (1010.md V18, ROADMAP_NEW R1): a
//! west–east vertical section through a storm, fused from several radars
//! ([`wxdata::multiradar`]). Three panels, top to bottom:
//! - the first radar alone;
//! - all of them fused;
//! - which radar counted most at each cell.
//!
//! Each cell is drawn in the reflectivity colour table where there is echo. A cell some radar
//! observed below threshold is a lighter grey than one no radar's beam reached. A text file
//! beside the picture gives the provenance colours and each radar's time offset.

use chrono::{DateTime, Utc};
use wxdata::level2::{self, Moment};
use wxdata::multiradar::{fuse, FusedVolume, Grid, RadarVolume};

/// Pixels per column and per level.
const COL_PX: u32 = 6;
const LEVEL_PX: u32 = 10;
/// Space between panels.
const GAP_PX: u32 = 8;
/// A cell no radar observed, one observed with no echo, and the gap between panels.
const UNSEEN: [u8; 4] = [24, 24, 30, 255];
const CLEAR: [u8; 4] = [62, 62, 74, 255];
const BACKGROUND: [u8; 4] = [10, 10, 14, 255];
/// The provenance panel's colour for each radar, in the order given.
const RADAR_COLOURS: [[u8; 4]; 6] = [
    [235, 235, 235, 255],
    [235, 125, 60, 255],
    [90, 170, 240, 255],
    [150, 220, 90, 255],
    [215, 120, 220, 255],
    [240, 210, 80, 255],
];

/// Fuse `sites` around `center` for `time` and write the section to `out_path`, and its legend
/// beside it as `.txt`.
pub fn run_fusion(
    sites: &[String],
    time: DateTime<Utc>,
    center: [f64; 2],
    out_path: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(!sites.is_empty(), "no radars given");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let mut held = Vec::new();
    for site in sites {
        let site = site.to_ascii_uppercase();
        let ids = rt.block_on(level2::list_volumes(&site, time.date_naive()))?;
        let Some(id) = ids
            .into_iter()
            .filter(|id| id.date_time().is_some())
            .min_by_key(|id| (id.date_time().unwrap() - time).num_seconds().abs())
        else {
            println!("{site}: no volume that day");
            continue;
        };
        let vol_time = id.date_time().unwrap();
        let scan = rt.block_on(level2::download_scan(id, crate::paths::cache_dir()))?;
        let sweeps: Vec<_> = (0..level2::elevation_angles(&scan).len())
            .filter_map(|t| {
                level2::bin_scan_opts_recorded(&scan, Moment::Reflectivity, t, false).ok()
            })
            .collect();
        let antenna_m =
            wxdata::sites::site_by_id(&site).map_or(0.0, |s| f64::from(s.elevation_meters));
        held.push((site, vol_time, antenna_m, sweeps));
    }
    let radars: Vec<RadarVolume<'_>> = held
        .iter()
        .map(|(site, t, antenna_m, sweeps)| RadarVolume {
            site: site.clone(),
            time: *t,
            antenna_m: *antenna_m,
            sweeps,
        })
        .collect();
    anyhow::ensure!(!radars.is_empty(), "none of the radars had a volume");
    // 60 km west to east through the centre, 0.5 km columns and levels to 18.5 km. The whole
    // box is fused; only its middle row is drawn.
    let grid = Grid {
        center,
        half_km: 30.0,
        n: 121,
        nz: 37,
        bottom_km: 0.5,
        top_km: 18.5,
    };
    let alone = fuse(&radars[..1], time, 300, grid);
    let all = fuse(&radars, time, 300, grid);
    let img = section_image(&grid, &[&alone, &all], &all);
    img.save(out_path)?;
    let legend = legend(&all, &radars[0].site);
    print!("{legend}");
    std::fs::write(std::path::Path::new(out_path).with_extension("txt"), legend)?;
    println!("wrote {out_path}");
    Ok(())
}

/// The middle row of each volume in `panels`, then `provenance`'s dominant radar, stacked.
fn section_image(
    grid: &Grid,
    panels: &[&FusedVolume],
    provenance: &FusedVolume,
) -> image::RgbaImage {
    let table = crate::colormap::default_table(Moment::Reflectivity);
    let (w, h) = (grid.n as u32 * COL_PX, grid.nz as u32 * LEVEL_PX);
    let count = panels.len() as u32 + 1;
    let mut img =
        image::RgbaImage::from_pixel(w, count * h + (count - 1) * GAP_PX, image::Rgba(BACKGROUND));
    let row = grid.n / 2;
    let mut fill = |panel: u32, i: usize, k: usize, rgba: [u8; 4]| {
        let x0 = i as u32 * COL_PX;
        // Level 0 at the bottom of the panel.
        let y0 = panel * (h + GAP_PX) + (grid.nz - 1 - k) as u32 * LEVEL_PX;
        for y in y0..y0 + LEVEL_PX {
            for x in x0..x0 + COL_PX {
                img.put_pixel(x, y, image::Rgba(rgba));
            }
        }
    };
    for (p, f) in panels.iter().enumerate() {
        for k in 0..grid.nz {
            for i in 0..grid.n {
                let at = grid.index(i, row, k);
                let v = f.dbz[at];
                let rgba = if !v.is_nan() {
                    table.sample(v).unwrap_or(CLEAR)
                } else if f.observed[at] != 0 {
                    CLEAR
                } else {
                    UNSEEN
                };
                fill(p as u32, i, k, rgba);
            }
        }
    }
    let last = panels.len() as u32;
    for k in 0..grid.nz {
        for i in 0..grid.n {
            let at = grid.index(i, row, k);
            let d = provenance.dominant[at];
            let rgba = if d != u8::MAX {
                RADAR_COLOURS[usize::from(d) % RADAR_COLOURS.len()]
            } else if provenance.observed[at] != 0 {
                CLEAR
            } else {
                UNSEEN
            };
            fill(last, i, k, rgba);
        }
    }
    // A faint line every 2 km of height across every panel.
    for p in 0..count {
        for k in (3..grid.nz).step_by(4) {
            let y = p * (h + GAP_PX) + (grid.nz - 1 - k) as u32 * LEVEL_PX + LEVEL_PX - 1;
            for x in 0..w {
                let px = img.get_pixel_mut(x, y);
                for c in 0..3 {
                    px.0[c] = px.0[c].saturating_add(28);
                }
            }
        }
    }
    img
}

/// What the picture shows, in words.
fn legend(f: &FusedVolume, first: &str) -> String {
    let g = f.grid;
    let mut s = format!(
        "West-east section through {:.3}, {:.3} at {}: {:.0} km wide, {:.1} to {:.1} km above sea level, \
         {:.1} km columns and levels; faint lines every 2 km of height.\n\
         Panels, top to bottom: {first} alone; all radars fused; the radar that counted most.\n\
         Dark: no radar's beam reached the cell. Grey: observed, no echo.\n",
        g.center[1],
        g.center[0],
        f.time.format("%Y-%m-%d %H:%M UTC"),
        2.0 * g.half_km,
        g.bottom_km,
        g.top_km,
        2.0 * g.half_km / (g.n - 1) as f64
    );
    for (i, site) in f.sites.iter().enumerate() {
        let c = RADAR_COLOURS[i % RADAR_COLOURS.len()];
        s.push_str(&format!(
            "{site}: volume {:+} s; provenance colour rgb({}, {}, {})\n",
            f.offsets_s[i], c[0], c[1], c[2]
        ));
    }
    for l in &f.left_out {
        s.push_str(&format!("{} left out: {}\n", l.site, l.reason));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picture has three panels of the grid's size, and a cell with no beam, one observed
    /// clear and one with echo each get their own colour.
    #[test]
    fn the_section_has_three_panels_and_tells_unseen_from_clear() {
        let grid = Grid {
            center: [-97.5, 35.3],
            half_km: 1.0,
            n: 3,
            nz: 2,
            bottom_km: 1.0,
            top_km: 2.0,
        };
        let len = grid.len();
        let mut f = FusedVolume {
            grid,
            time: chrono::DateTime::from_timestamp(1_369_080_000, 0).unwrap(),
            sites: vec!["KTLX".into()],
            offsets_s: vec![0],
            left_out: Vec::new(),
            motion_ms: None,
            dbz: vec![f32::NAN; len],
            observed: vec![0; len],
            echo: vec![0; len],
            dominant: vec![u8::MAX; len],
        };
        let row = grid.n / 2;
        let echo_at = grid.index(0, row, 0);
        f.dbz[echo_at] = 50.0;
        f.observed[echo_at] = 1;
        f.echo[echo_at] = 1;
        f.dominant[echo_at] = 0;
        f.observed[grid.index(1, row, 0)] = 1;
        let img = section_image(&grid, &[&f, &f], &f);
        let (w, h) = (grid.n as u32 * COL_PX, grid.nz as u32 * LEVEL_PX);
        assert_eq!((img.width(), img.height()), (w, 3 * h + 2 * GAP_PX));
        // Bottom level of the top panel: echo, clear, unseen, left to right.
        let y = h - LEVEL_PX / 2;
        let at = |i: u32| img.get_pixel(i * COL_PX + COL_PX / 2, y).0;
        assert_ne!(at(0), CLEAR);
        assert_ne!(at(0), UNSEEN);
        assert_eq!(at(1), CLEAR);
        assert_eq!(at(2), UNSEEN);
        // The provenance panel names the radar.
        let y = 3 * h + 2 * GAP_PX - LEVEL_PX / 2;
        assert_eq!(img.get_pixel(COL_PX / 2, y).0, RADAR_COLOURS[0]);
        assert!(legend(&f, "KTLX").contains("KTLX: volume +0 s"));
    }
}
