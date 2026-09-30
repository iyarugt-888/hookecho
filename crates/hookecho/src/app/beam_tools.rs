//! Terrain and beam tools: beam blockage, the lowest usable tilt, two-radar coverage comparison,
//! and CAPPI slices. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Keep the beam-blockage raster in step with the camera, the site, and the displayed tilt.
    ///
    /// Building one fetches DEM tiles, so it runs as a task and lands on `blockage_rx`. The old
    /// raster keeps painting (stretched to its own world rect) until the new one arrives, which is
    /// what makes a pan look continuous instead of blinking.
    pub(crate) fn update_blockage(&mut self, ctx: &egui::Context) {
        while let Ok((key, world, image)) = self.blockage_rx.try_recv() {
            let tex = ctx.load_texture("blockage", image, egui::TextureOptions::LINEAR);
            self.blockage_tex = Some((key, tex, world));
            self.blockage_pending = None;
        }
        if !self.show_blockage {
            self.blockage_tex = None;
            self.blockage_pending = None;
            return;
        }
        let view = &self.views[self.active];
        let (Some(site), Some(vol)) = (
            view.site.as_deref().and_then(wxdata::sites::site_by_id),
            view.volume.as_ref(),
        ) else {
            return;
        };
        let Some(tilt_deg) = vol.elevations.get(view.tilt).copied() else {
            return;
        };
        let cam = &view.camera;
        let vp = self.last_viewport;
        let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
        let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
        let world = [wx0, wy0, wx1, wy1];
        let key = BlockageKey {
            site: site.id.to_string(),
            tilt_mdeg: (tilt_deg * 1000.0) as i32,
            world: world.map(|w| (w * 1e7) as i64),
        };
        if self.blockage_tex.as_ref().is_some_and(|(k, ..)| *k == key)
            || self.blockage_pending.as_ref().is_some_and(|(k, at)| {
                *k == key || at.elapsed() < std::time::Duration::from_millis(600)
            })
        {
            return;
        }
        let beam = crate::elevation::BeamSite {
            lon: site.longitude as f64,
            lat: site.latitude as f64,
            // The site registry is the elevation source; the tower table adds the antenna.
            ground_m: site.elevation_meters as f64,
            tower_m: wxdata::towers::tower_m(site.id),
        };
        self.blockage_pending = Some((key.clone(), Instant::now()));
        let http = self.http.clone();
        let tx = self.blockage_tx.clone();
        let ctx = ctx.clone();
        let tilt_deg64 = tilt_deg as f64;
        self.spawner.spawn(async move {
            let image = crate::elevation::blockage_image(&http, beam, tilt_deg64, world).await;
            let _ = tx.send((key, world, image));
            ctx.request_repaint();
        });
    }

    /// Keep the lowest-usable-tilt raster in step with the camera, the site, and the volume's own
    /// elevation list — see `update_blockage`, which this otherwise mirrors exactly.
    pub(crate) fn update_lowest_tilt(&mut self, ctx: &egui::Context) {
        while let Ok((key, world, image)) = self.lowest_tilt_rx.try_recv() {
            let tex = ctx.load_texture("lowest_tilt", image, egui::TextureOptions::LINEAR);
            self.lowest_tilt_tex = Some((key, tex, world));
            self.lowest_tilt_pending = None;
        }
        if !self.show_lowest_tilt {
            self.lowest_tilt_tex = None;
            self.lowest_tilt_pending = None;
            return;
        }
        let view = &self.views[self.active];
        let (Some(site), Some(vol)) = (
            view.site.as_deref().and_then(wxdata::sites::site_by_id),
            view.volume.as_ref(),
        ) else {
            return;
        };
        if vol.elevations.is_empty() {
            return;
        }
        let cam = &view.camera;
        let vp = self.last_viewport;
        let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
        let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
        let world = [wx0, wy0, wx1, wy1];
        // Ascending, and deduplicated the same way the cross-section's beam-rise lines are: a
        // SAILS/MRLE repeat of a low cut must not count as two separate rungs on the color ramp.
        let mut tilts_deg: Vec<f32> = vol.elevations.clone();
        tilts_deg.sort_by(f32::total_cmp);
        tilts_deg.dedup_by(|a, b| (*a - *b).abs() < 0.05);
        let key = LowestTiltKey {
            site: site.id.to_string(),
            tilts_mdeg: tilts_deg.iter().map(|&t| (t * 1000.0) as i32).collect(),
            world: world.map(|w| (w * 1e7) as i64),
        };
        if self
            .lowest_tilt_tex
            .as_ref()
            .is_some_and(|(k, ..)| *k == key)
            || self.lowest_tilt_pending.as_ref().is_some_and(|(k, at)| {
                *k == key || at.elapsed() < std::time::Duration::from_millis(600)
            })
        {
            return;
        }
        let beam = crate::elevation::BeamSite {
            lon: site.longitude as f64,
            lat: site.latitude as f64,
            ground_m: site.elevation_meters as f64,
            tower_m: wxdata::towers::tower_m(site.id),
        };
        self.lowest_tilt_pending = Some((key.clone(), Instant::now()));
        let http = self.http.clone();
        let tx = self.lowest_tilt_tx.clone();
        let ctx = ctx.clone();
        let tilts: Vec<f64> = tilts_deg.iter().map(|&t| t as f64).collect();
        self.spawner.spawn(async move {
            let image =
                crate::elevation::lowest_usable_tilt_image(&http, beam, &tilts, world).await;
            let _ = tx.send((key, world, image));
            ctx.request_repaint();
        });
    }

    /// Keep the coverage-comparison raster in step with the camera and the chosen site pair.
    /// Pure geometry (`crate::coverage_compare::coverage_compare_image`) rather than a DEM fetch,
    /// so unlike `update_blockage`/`update_lowest_tilt` this rebuilds inline on the UI thread —
    /// a 256×256 raster of beam-height arithmetic is well under a frame budget.
    pub(crate) fn update_coverage_compare(&mut self, ctx: &egui::Context) {
        let Some((a_id, b_id)) = self.coverage_compare.clone() else {
            self.coverage_compare_tex = None;
            return;
        };
        let (Some(site_a), Some(site_b)) = (
            wxdata::sites::site_by_id(&a_id),
            wxdata::sites::site_by_id(&b_id),
        ) else {
            self.coverage_compare = None;
            self.coverage_compare_tex = None;
            return;
        };
        let view = &self.views[self.active];
        let cam = &view.camera;
        let vp = self.last_viewport;
        let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
        let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
        let world = [wx0, wy0, wx1, wy1];
        let key = CoverageCompareKey {
            site_a: site_a.id.to_string(),
            site_b: site_b.id.to_string(),
            world: world.map(|w| (w * 1e7) as i64),
        };
        if self
            .coverage_compare_tex
            .as_ref()
            .is_some_and(|(k, ..)| *k == key)
        {
            return;
        }
        // 0.5°: the same fixed elevation the suitability popup itself ranks candidates at, so
        // this overlay always agrees with the "Beam height" numbers the user just read there.
        let image = crate::coverage_compare::coverage_compare_image(site_a, site_b, 0.5, world);
        let tex = ctx.load_texture("coverage_compare", image, egui::TextureOptions::LINEAR);
        self.coverage_compare_tex = Some((key, tex, world));
    }

    /// Re-slice the active pane's cached volume into a CAPPI at `cappi_alt_km` when the key
    /// (volume name + altitude) changed, and refresh the window texture (feature AA).
    pub(crate) fn update_cappi(&mut self, ctx: &egui::Context) {
        const N: usize = 256;
        let Some(name) = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.name.clone())
        else {
            self.cappi_tex = None;
            self.cappi_key = None;
            return;
        };
        let key = (name, self.cappi_alt_km.to_bits());
        if self.cappi_key.as_ref() == Some(&key) {
            return;
        }
        let Some(vol) = self.views[self.active].volume.as_mut() else {
            return;
        };
        let sweeps = vol.reflectivity_tilts();
        if sweeps.is_empty() {
            return;
        }
        // Same fix as `build_volume3d`: slice out to what this scan actually sampled, not a
        // fixed radius that cut the CAPPI off well short of far reflectivity returns.
        let half_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
        let Some(c) = wxdata::volume3d::cappi(&sweeps, self.cappi_alt_km, N, half_km) else {
            return;
        };
        let hc_table = crate::colormap::effective_table(
            &self.palettes,
            Moment::Reflectivity,
            self.settings.theme,
        );
        let img = ui::cappi_window::to_image(&c, &hc_table);
        self.cappi_tex = Some(ctx.load_texture("cappi", img, egui::TextureOptions::NEAREST));
        self.cappi_key = Some(key);
    }
}
