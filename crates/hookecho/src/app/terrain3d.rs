//! Terrain in the 3D map (ROADMAP_NEW H5): the ground itself as a shaded surface at its height,
//! so beams, cores and surfaces can be read against the hills they pass over and the ridges that
//! block them. Heights are the same AWS terrarium DEM the blockage tools read
//! ([`crate::elevation`]), sampled over the view in the background and redrawn every frame at the
//! 3D map's vertical exaggeration.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;

/// Samples per side of the terrain grid.
const GRID: usize = 150;
/// The sampled box follows the view in steps of this many degrees, so small pans reuse it.
const STEP_DEG: f64 = 0.5;
/// The largest box sampled, degrees either side of the view's centre.
const MAX_HALF_DEG: f64 = 3.0;

type Key = (i32, i32, i32, i32);

#[derive(Default)]
pub(crate) struct Terrain3d {
    key: Option<Key>,
    pub(crate) grid: Option<wxdata::mrms::MrmsField>,
    rx: Option<std::sync::mpsc::Receiver<Option<wxdata::mrms::MrmsField>>>,
}

/// Hypsometric colour for a height (km MSL): lowland green, through tan and brown, to grey rock.
pub(crate) fn terrain_color(km: f32) -> [u8; 3] {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [70.0, 110.0, 70.0]),
        (0.5, [120.0, 135.0, 80.0]),
        (1.2, [160.0, 135.0, 95.0]),
        (2.5, [125.0, 100.0, 80.0]),
        (4.0, [200.0, 200.0, 205.0]),
    ];
    let km = km.max(0.0);
    let i = STOPS.iter().rposition(|(h, _)| *h <= km).unwrap_or(0);
    let (h0, c0) = STOPS[i];
    let (h1, c1) = STOPS[(i + 1).min(STOPS.len() - 1)];
    let t = if h1 > h0 {
        ((km - h0) / (h1 - h0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [0, 1, 2].map(|k| (c0[k] + t * (c1[k] - c0[k])) as u8)
}

impl HookEchoApp {
    /// While a pane shows 3D terrain, keep a height grid over its view.
    pub(crate) fn sync_terrain3d(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.terrain3d.rx {
            match rx.try_recv() {
                Ok(grid) => {
                    self.terrain3d.rx = None;
                    self.terrain3d.grid = grid;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.terrain3d.rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        let v = &self.views[self.active];
        if !v.map_3d.enabled || !v.map_3d.terrain {
            return;
        }
        // Centred on the camera, not on the view's corner bounds: a pitched view's far corners
        // reach toward the horizon, and their midpoint slid the box off to one side.
        let (w, s, e, n) = self.view_bounds();
        let cam = &self.views[self.active].camera;
        let (clon, clat) = crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
        let half_lon = ((e - w) * 0.5).clamp(0.5, MAX_HALF_DEG);
        let half_lat = ((n - s) * 0.5).clamp(0.5, MAX_HALF_DEG);
        let snap = |x: f64| (x / STEP_DEG).round() as i32;
        let key = (
            snap(clon - half_lon),
            snap(clat - half_lat),
            snap(clon + half_lon),
            snap(clat + half_lat),
        );
        if self.terrain3d.key == Some(key) {
            return;
        }
        self.terrain3d.key = Some(key);
        let [w, s, e, n] = [key.0, key.1, key.2, key.3].map(|k| k as f64 * STEP_DEG);
        let (tx, rx) = std::sync::mpsc::channel();
        self.terrain3d.rx = Some(rx);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            // z7: ~1.2 km a pixel, finer than the grid's few-km spacing, and a handful of tiles.
            let values: Vec<f32> =
                crate::elevation::elevation_grid(&http, [w, s, e, n], GRID, GRID, 7)
                    .await
                    .into_iter()
                    // Sea and the sea floor sit at sea level: the surface is the ground.
                    .map(|m| if m.is_nan() { m } else { m.max(0.0) / 1000.0 })
                    .collect();
            let any = values.iter().any(|v| !v.is_nan());
            let grid = any.then(|| wxdata::mrms::MrmsField {
                values,
                nx: GRID,
                ny: GRID,
                lon_west: w,
                lon_east: e,
                lat_north: n,
                lat_south: s,
                time: chrono::Utc::now(),
            });
            let _ = tx.send(grid);
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::terrain_color;

    #[test]
    fn terrain_runs_green_to_rock_with_height() {
        let low = terrain_color(0.1);
        let high = terrain_color(4.5);
        assert!(low[1] > low[0], "lowland is green: {low:?}");
        assert!(
            high.iter().all(|c| *c > 180),
            "high ground is pale rock: {high:?}"
        );
        assert_eq!(
            terrain_color(-0.2),
            terrain_color(0.0),
            "below sea level is sea level"
        );
    }
}
