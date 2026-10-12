//! The 3D window with nearby radars added (ROADMAP_NEW R1, 1010.md V18): its "Add nearby
//! radars" switch fuses the active pane's reflectivity with up to three other WSR-88Ds over a
//! box 80 km across around the map's centre ([`wxdata::multiradar::fuse`]), and shows that box
//! instead of the single-radar volume while the switch is on. The fused grid (1 km columns,
//! 0.5 km levels) is resampled onto the window's own grid, so the renderer and its controls are
//! the same. The added radars are fetched and fused off the UI thread where there is one, the
//! same way the cross-section's are (`xsection_fuse`). Reflectivity only.

use super::xsection_fuse::{bin_added, fetch_added, nearby_radars, MAX_ADDED, MAX_OFFSET_S};
use super::*;

/// The fused box's half-width, km.
const HALF_KM: f64 = 40.0;
/// Columns across it (1 km apart) and levels up to the window's top (0.5 km apart).
const COLS: usize = 81;
const LEVELS: usize = 37;

/// What a fused box was asked for: the volume, and the box's centre to the nearest 0.05°.
type FusionKey = (String, [i32; 2]);

type FusionReply = (
    FusionKey,
    Result<(crate::render3d::Volume3dUpload, String), String>,
);

#[derive(Default)]
pub(crate) struct Vol3dFusion {
    rx: Option<std::sync::mpsc::Receiver<FusionReply>>,
    pending: Option<FusionKey>,
    /// The last request started, so a view that has stopped moving is fused once.
    attempted: Option<FusionKey>,
    /// The volume whose fused box the window shows.
    showing: Option<String>,
}

/// `[lon, lat]` to the key's grid: the box is refused only when the view moves 0.05°.
fn centre_key(lonlat: [f64; 2]) -> [i32; 2] {
    [
        (lonlat[0] * 20.0).round() as i32,
        (lonlat[1] * 20.0).round() as i32,
    ]
}

/// The fused box resampled onto an `n`×`n`×`nz` grid of the window's (nearest cell), as an
/// upload: codes 2..=255 across `range` as the single-radar build makes them, 0 where no radar
/// saw echo. Levels count up from the pane radar's antenna, as the window's do.
fn fused_upload(
    fused: &wxdata::multiradar::FusedVolume,
    grid: wxdata::multiradar::Grid,
    range: (f32, f32),
    lut: Vec<u8>,
    center_km: [f32; 2],
    (n, nz, top_km): (usize, usize, f32),
) -> crate::render3d::Volume3dUpload {
    let span = (range.1 - range.0).max(f32::EPSILON);
    let nearest = |m: usize, count: usize, to: usize| {
        ((m as f64 / (count - 1).max(1) as f64) * (to - 1) as f64).round() as usize
    };
    let mut data = vec![0u8; n * n * nz];
    for k in 0..nz {
        let ks = nearest(k, nz, grid.nz);
        for j in 0..n {
            let js = nearest(j, n, grid.n);
            for i in 0..n {
                let v = fused.dbz[grid.index(nearest(i, n, grid.n), js, ks)];
                if !v.is_nan() {
                    let t = ((v - range.0) / span).clamp(0.0, 1.0);
                    data[i + n * j + n * n * k] = 2 + (t * 253.0) as u8;
                }
            }
        }
    }
    crate::render3d::Volume3dUpload {
        data: crate::render3d::pack_rg8(&data),
        n: n as u32,
        nz: nz as u32,
        lut,
        half_km: grid.half_km as f32,
        center_km,
        top_km,
        outside: 0.0,
        value_range: None,
        lut_range: None,
    }
}

/// The window's line about a fused box.
fn fused_status(f: &wxdata::multiradar::FusedVolume, centre: [f64; 2]) -> String {
    let added: Vec<String> = f
        .sites
        .iter()
        .zip(&f.offsets_s)
        .skip(1)
        .map(|(s, o)| format!("{s} ({o:+} s)"))
        .collect();
    let mut s = if added.is_empty() {
        "No nearby radar had a volume close enough in time; the box is this radar's alone".into()
    } else {
        format!(
            "80 km around {:.2}, {:.2}, fused with {}: reflectivity weighted by each beam's \
             thickness. Layers and source below are this radar's.",
            centre[1],
            centre[0],
            added.join(", ")
        )
    };
    for l in &f.left_out {
        s.push_str(&format!(" {} left out: {}.", l.site, l.reason));
    }
    s
}

impl HookEchoApp {
    /// Whether the 3D window shows a fused box for the active pane's volume, so the
    /// single-radar build does not replace it.
    pub(crate) fn vol3d_fused_shown(&self) -> bool {
        self.vol3d.fuse
            && self.vol3d_fusion.showing.as_deref()
                == self.views[self.active]
                    .volume
                    .as_ref()
                    .map(|v| v.name.as_str())
    }

    /// Per frame while the 3D window is open: take a finished fusion, and with the switch on ask
    /// for one when the volume or the view's centre has changed.
    pub(crate) fn sync_vol3d_fusion(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.vol3d_fusion.rx {
            if let Ok((key, result)) = rx.try_recv() {
                self.vol3d_fusion.rx = None;
                let current = self.views[self.active]
                    .volume
                    .as_ref()
                    .map(|v| v.name.clone());
                if self.vol3d_fusion.pending.take().as_ref() == Some(&key)
                    && self.vol3d.fuse
                    && current.as_deref() == Some(key.0.as_str())
                {
                    match result {
                        Ok((upload, status)) => {
                            self.vol3d.half_km = upload.half_km;
                            self.vol3d_pending = Some(upload);
                            self.vol3d_fusion.showing = Some(key.0);
                            self.vol3d.fuse_status = Some(status);
                        }
                        Err(e) => {
                            self.vol3d.fuse_status = Some(format!("Nearby radars not added: {e}"))
                        }
                    }
                }
            }
        }
        if !self.show_3d {
            return;
        }
        if !self.vol3d.fuse {
            if self.vol3d_fusion.showing.take().is_some() {
                // Back to the single-radar volume: rebuild it, since its upload was replaced.
                self.vol3d.retry = true;
            }
            self.vol3d.fuse_status = None;
            self.vol3d_fusion.attempted = None;
            return;
        }
        if self.vol3d_fusion.rx.is_some() {
            return;
        }
        let view = &self.views[self.active];
        let (Some(site), Some(volume)) = (view.site.clone(), view.volume.as_ref()) else {
            return;
        };
        let c = view.camera.center;
        let (lon, lat) = crate::render::mercator::world_to_lonlat(c.0, c.1);
        let key: FusionKey = (volume.name.clone(), centre_key([lon, lat]));
        if self.vol3d_fusion.attempted.as_ref() == Some(&key) {
            return;
        }
        self.vol3d_fusion.attempted = Some(key.clone());
        let time = volume.time;
        let sweeps = self.views[self.active]
            .volume
            .as_mut()
            .expect("checked above")
            .reflectivity_tilts();
        let Some(s0) = sweeps.first() else {
            self.vol3d.fuse_status = Some("This volume has no reflectivity tilts".into());
            return;
        };
        let (rlon, rlat) = (f64::from(s0.radar_lon), f64::from(s0.radar_lat));
        // The box's centre east and north of the radar, km (flat over a few hundred km).
        let km_per_deg = 6371.0f64.to_radians();
        let center_km = [
            ((lon - rlon) * km_per_deg * rlat.to_radians().cos()) as f32,
            ((lat - rlat) * km_per_deg) as f32,
        ];
        let ground_km = f64::from(center_km[0]).hypot(f64::from(center_km[1]));
        if ground_km > 230.0 {
            self.vol3d.fuse_status = Some(format!(
                "The map's centre is {ground_km:.0} km from {site}; centre it within 230 km"
            ));
            return;
        }
        let others: Vec<String> = nearby_radars(lon, lat, &site, MAX_ADDED)
            .into_iter()
            .map(str::to_string)
            .collect();
        if others.is_empty() {
            self.vol3d.fuse_status =
                Some("No other radar within 230 km of the map's centre".into());
            return;
        }
        self.vol3d.fuse_status = Some(format!("Adding {}\u{2026}", others.join(", ")));
        self.vol3d_fusion.pending = Some(key.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        self.vol3d_fusion.rx = Some(rx);
        let range = (s0.value_min, s0.value_max);
        let table = crate::colormap::effective_table(
            &self.palettes,
            Moment::Reflectivity,
            self.settings.theme,
        );
        let lut = crate::colormap::bake_lut(&table, range, None).to_vec();
        let own_antenna = super::xsection_fuse::antenna_m(&site);
        let cache = crate::paths::cache_dir();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let scans = fetch_added(others, time, cache, "3D fusion").await;
            let result = wxdata::task::blocking(move || {
                let added = bin_added(scans);
                let mut radars = vec![wxdata::multiradar::RadarVolume {
                    site,
                    time,
                    antenna_m: own_antenna,
                    sweeps: &sweeps,
                }];
                radars.extend(added.iter().map(|(site, t, antenna_m, tilts)| {
                    wxdata::multiradar::RadarVolume {
                        site: site.clone(),
                        time: *t,
                        antenna_m: *antenna_m,
                        sweeps: tilts,
                    }
                }));
                // Levels from the pane radar's antenna up, as the window's are.
                let bottom = own_antenna / 1e3;
                let grid = wxdata::multiradar::Grid {
                    center: [lon, lat],
                    half_km: HALF_KM,
                    n: COLS,
                    nz: LEVELS,
                    bottom_km: bottom,
                    top_km: bottom + f64::from(VOL3D_TOP_KM),
                };
                let fused = wxdata::multiradar::fuse(&radars, time, MAX_OFFSET_S, grid);
                let status = fused_status(&fused, [lon, lat]);
                let upload = fused_upload(
                    &fused,
                    grid,
                    range,
                    lut,
                    center_km,
                    (VOL3D_N, VOL3D_NZ, VOL3D_TOP_KM),
                );
                (upload, status)
            })
            .await
            .map_err(|e| e.to_string());
            let _ = tx.send((key, result));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fused box resampled onto the window's grid keeps each cell's echo where it was, and
    /// leaves unseen cells empty.
    #[test]
    fn the_fused_box_lands_on_the_windows_grid() {
        let grid = wxdata::multiradar::Grid {
            center: [-97.5, 35.3],
            half_km: 40.0,
            n: 3,
            nz: 2,
            bottom_km: 0.4,
            top_km: 18.4,
        };
        let mut dbz = vec![f32::NAN; grid.len()];
        // The east column of the middle row, at the bottom: 50 dBZ.
        dbz[grid.index(2, 1, 0)] = 50.0;
        let fused = wxdata::multiradar::FusedVolume {
            grid,
            time: chrono::Utc::now(),
            sites: vec!["KTLX".into()],
            offsets_s: vec![0],
            left_out: Vec::new(),
            motion_ms: None,
            dbz,
            observed: vec![0; grid.len()],
            echo: vec![0; grid.len()],
            dominant: vec![u8::MAX; grid.len()],
        };
        let up = fused_upload(
            &fused,
            grid,
            (0.0, 100.0),
            vec![0; 4],
            [3.0, -2.0],
            (5, 3, 18.0),
        );
        let at = |i: usize, j: usize, k: usize| up.data[2 * (i + 5 * j + 25 * k)];
        assert_eq!(at(4, 2, 0), 2 + (0.5 * 253.0) as u8, "east, middle, bottom");
        assert_eq!(
            at(3, 2, 0),
            2 + (0.5 * 253.0) as u8,
            "nearest to the east column"
        );
        assert_eq!(at(0, 2, 0), 0, "west: unseen");
        assert_eq!(at(4, 2, 2), 0, "top: unseen");
        assert_eq!((up.half_km, up.center_km), (40.0, [3.0, -2.0]));
        assert_eq!(centre_key([-97.5, 35.3]), [-1950, 706]);
    }
}
