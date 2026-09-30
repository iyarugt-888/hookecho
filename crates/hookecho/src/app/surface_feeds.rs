//! Surface and point feeds kept in step with the view: stations, METARs, gauges, webcams, the
//! mosaic and cloud tops, and opening a webcam or damage point. Moved out of `app.rs` unchanged
//! (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Drive the METAR station-plot fetch (feature U): only when enabled and zoomed in enough,
    /// refetching every 75 s or when the view center drifts out of the fetched bbox's middle half.
    pub(crate) fn sync_metar(&mut self, ctx: &egui::Context) {
        // Pilot reports share this function's view-bbox logic but not its zoom gate: they are
        // sparse enough to plot nationwide, and on their own two-minute clock.
        if self.show_pireps
            && self
                .pirep_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 120)
        {
            let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
            self.pirep_last_fetch = Some(Instant::now());
            self.spawn_overlay(
                ctx,
                OverlaySource::Pireps(min_lat, min_lon, max_lat, max_lon),
            );
        }
        if !self.show_metar {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        if (max_lon - min_lon) > 12.0 {
            return; // too zoomed out — a nationwide plot would be unreadable and huge
        }
        let (clon, clat) = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        let stale = self
            .metar_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 75);
        // Refetch when the center leaves the middle half of the last fetched bbox.
        let drifted = self.metar_bounds.is_none_or(|(la0, lo0, la1, lo1)| {
            let (mlon, mlat) = ((lo0 + lo1) * 0.5, (la0 + la1) * 0.5);
            let (hw, hh) = ((lo1 - lo0) * 0.25, (la1 - la0) * 0.25);
            (clon - mlon).abs() > hw || (clat - mlat).abs() > hh
        });
        if stale || drifted {
            // Pad the fetch bbox 20% past the view, clamped to 15° per side.
            let pad_lon = ((max_lon - min_lon) * 0.2).min(15.0);
            let pad_lat = ((max_lat - min_lat) * 0.2).min(15.0);
            let (lat0, lon0) = (min_lat - pad_lat, min_lon - pad_lon);
            let (lat1, lon1) = (max_lat + pad_lat, max_lon + pad_lon);
            self.metar_last_fetch = Some(Instant::now());
            self.metar_bounds = Some((lat0, lon0, lat1, lon1));
            self.spawn_overlay(ctx, OverlaySource::Metar(lat0, lon0, lat1, lon1));
        }
    }

    pub(crate) fn sync_mosaic(&mut self, ctx: &egui::Context) {
        use crate::render::FieldLayer as FL;
        if !self.field_wanted(FL::Mosaic) {
            return;
        }
        if !self.views[self.active].timeline.following {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        let cap = if cfg!(target_os = "android") { 4 } else { 6 };
        // A metered link pays per site; halve the composite rather than drop it.
        let cap = if crate::platform::is_metered() {
            cap / 2
        } else {
            cap
        };
        let sites = wxdata::mosaic::sites_for_view(
            self.views[self.active].site.as_deref(),
            min_lon,
            min_lat,
            max_lon,
            max_lat,
            cap,
        );
        if sites.is_empty() {
            return;
        }
        let stale = self.fields.get(&FL::Mosaic).is_some_and(|s| {
            s.last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::Mosaic))
        });
        let moved = self.mosaic_bounds != Some((min_lon, min_lat, max_lon, max_lat))
            && sites != self.mosaic_sites;
        if stale || moved {
            if let Some(s) = self.fields.get_mut(&FL::Mosaic) {
                s.last_fetch = Some(Instant::now());
            }
            self.mosaic_bounds = Some((min_lon, min_lat, max_lon, max_lat));
            self.spawn_overlay(ctx, OverlaySource::Mosaic(sites));
        }
    }

    pub(crate) fn sync_webcams(&mut self, ctx: &egui::Context) {
        if !self.show_webcams {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        if (max_lon - min_lon) > 20.0 {
            return; // zoomed out past the point where individual cameras mean anything
        }
        let (clon, clat) = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        // Under ten minutes on purpose: Windy's free-tier image URLs carry a token that expires at
        // exactly ten, so a 600 s clock would race it and serve 401s.
        let stale = self
            .webcam_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 480);
        let drifted = self.webcam_bounds.is_none_or(|(lo0, la0, lo1, la1)| {
            let (mlon, mlat) = ((lo0 + lo1) * 0.5, (la0 + la1) * 0.5);
            let (hw, hh) = ((lo1 - lo0) * 0.25, (la1 - la0) * 0.25);
            (clon - mlon).abs() > hw || (clat - mlat).abs() > hh
        });
        if stale || drifted {
            let pad_lon = ((max_lon - min_lon) * 0.25).min(10.0);
            let pad_lat = ((max_lat - min_lat) * 0.25).min(10.0);
            let b = (
                min_lon - pad_lon,
                min_lat - pad_lat,
                max_lon + pad_lon,
                max_lat + pad_lat,
            );
            self.webcam_last_fetch = Some(Instant::now());
            self.webcam_bounds = Some(b);
            self.spawn_overlay(
                ctx,
                OverlaySource::Webcams(b.0, b.1, b.2, b.3, self.settings.windy_key.clone()),
            );
        }
    }

    /// Drive the live-station layer: poll the networks on a clock, refresh the electric field
    /// far more slowly (the model publishes every five minutes), and pull the camera catalog only
    /// when the view has moved somewhere new.
    ///
    /// The poll is what fills every open card's ring buffer, so it keeps running while any card is
    /// open even if the layer itself has been switched off.
    pub(crate) fn sync_stations(&mut self, ctx: &egui::Context) {
        if !self.show_stations && self.stations.cards.is_empty() {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        // A continental view would ask for thousands of stations to draw a dot each; the cards are
        // a close-in tool, so the layer waits until the view is regional.
        if (max_lon - min_lon) > 20.0 {
            return;
        }
        if self
            .station_last_poll
            .is_none_or(|t| t.elapsed().as_secs() >= 60)
        {
            self.station_last_poll = Some(Instant::now());
            // Still-only cameras (every camera, on a phone) get a fresh frame on the same clock.
            let (rt, http) = (self.spawner.clone(), self.http.clone());
            self.stations.refresh_stills(&rt, &http, ctx);
            self.spawn_overlay(
                ctx,
                OverlaySource::Stations {
                    bbox: (min_lat, min_lon, max_lat, max_lon),
                    center: ((min_lat + max_lat) * 0.5, (min_lon + max_lon) * 0.5),
                    tempest: self.settings.tempest_token.clone(),
                    wu: self.settings.wu_key.clone(),
                    synoptic: self.settings.synoptic_token.clone(),
                },
            );
            if !self.settings.field_mill_url.is_empty() {
                self.spawn_overlay(
                    ctx,
                    OverlaySource::Mill(self.settings.field_mill_url.clone()),
                );
            }
        }
        if self
            .ppef_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 300)
        {
            self.ppef_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Ppef);
        }
        // The camera catalog is megabytes of slow-changing agency data: fetch it per view box, not
        // per tick.
        let bbox = (
            (min_lon * 2.0).round() / 2.0,
            (min_lat * 2.0).round() / 2.0,
            (max_lon * 2.0).round() / 2.0,
            (max_lat * 2.0).round() / 2.0,
        );
        if self.dotcam_bounds != Some(bbox) {
            self.dotcam_bounds = Some(bbox);
            self.spawn_overlay(ctx, OverlaySource::DotCams(bbox.0, bbox.1, bbox.2, bbox.3));
        }
    }

    /// Open a camera site's detail popup and start pulling its newest frame.
    ///
    /// The image rides the placefile-icon texture cache: same fetch, same decode, same upload, and
    /// the key is known before the fetch resolves, so the window can show a placeholder and swap
    /// the picture in when it lands.
    pub(crate) fn open_webcam(&mut self, site: &wxdata::webcams::CamSite, ctx: &egui::Context) {
        let mut body = String::new();
        if !site.icao.is_empty() {
            body.push_str(&format!("{} ({})\n", site.icao, site.ident));
        }
        for c in &site.cameras {
            let state = if c.out_of_order {
                "  (out of order)"
            } else {
                ""
            };
            body.push_str(&format!("{}  {}{}\n", c.name, c.direction, state));
        }
        // Which network this came from, and the credit each one is owed. Windy's terms require a
        // visible "Webcams provided by Windy.com" wherever their cameras appear.
        let from_windy = site.link.is_some();
        body.push_str(if from_windy {
            "\nWebcams provided by Windy.com"
        } else {
            "\nFAA WeatherCams"
        });
        // The first working camera is the one we show; the rest are listed above.
        let cam = site.cameras.iter().find(|c| !c.out_of_order);
        let key = cam.map(|c| format!("cam:{}", c.id));
        self.detail = Some(Detail {
            title: format!("{} webcam", site.name),
            body,
            color: [110, 180, 240, 255],
            image: key.clone(),
            // Not decoration: the link back to the camera's own page is a condition of using
            // Windy's images at all.
            link: site
                .link
                .clone()
                .map(|u| ("View on Windy.com".to_string(), u)),
        });
        let (Some(key), Some(cam)) = (key, cam) else {
            return;
        };
        if self.pf_icon_tex.contains_key(&key) {
            return; // already loaded, or a fetch is already in flight
        }
        self.pf_icon_tex.insert(key.clone(), None);
        let http = self.http.clone();
        let tx = self.pf_icon_tx.clone();
        let ctx2 = ctx.clone();
        // Windy hands back the still's URL inline, so only the FAA needs a second round trip.
        let inline = cam.image_url.clone();
        let cam_id = cam.id;
        self.spawner.spawn(async move {
            let url = match inline {
                Some(u) => Some(u),
                None => match wxdata::webcams::latest_image(&http, cam_id).await {
                    Ok(u) => u,
                    Err(e) => {
                        log::warn!("webcam {cam_id} lookup failed: {e}");
                        None
                    }
                },
            };
            let Some(url) = url else {
                log::info!("camera {cam_id} has no recent image");
                return;
            };
            match fetch_icon_sheet(&http, &url).await {
                Ok(image) => {
                    let _ = tx.send((key, image));
                    ctx2.request_repaint();
                }
                Err(e) => log::warn!("webcam image {url} failed: {e}"),
            }
        });
    }

    /// Open a damage-survey point's detail popup, pulling its survey photo when one was attached.
    /// Shares the placefile-icon texture cache with the webcam popup.
    pub(crate) fn open_damage_point(&mut self, p: &wxdata::dat::DamagePoint, ctx: &egui::Context) {
        let mut body = String::new();
        if !p.damage.is_empty() {
            body.push_str(&format!("{}\n", p.damage));
        }
        if !p.dod.is_empty() {
            body.push_str(&format!("{}\n", p.dod));
        }
        if let Some(w) = p.windspeed {
            body.push_str(&format!("Estimated wind: {w} mph\n"));
        }
        if p.deaths > 0 || p.injuries > 0 {
            body.push_str(&format!("Deaths {} · injuries {}\n", p.deaths, p.injuries));
        }
        if let Some(t) = p.storm {
            body.push_str(&format!("Storm: {}\n", t.format("%Y-%m-%d %H:%M UTC")));
        }
        if let Some(c) = &p.comments {
            body.push_str(&format!("\n{c}\n"));
        }
        body.push_str(&format!("\nNWS {} survey (DAT)", p.office));
        let key = p.image.as_ref().map(|url| format!("dat:{url}"));
        let color = ef_color(&p.efscale).to_array();
        self.detail = Some(Detail {
            title: format!(
                "{} damage",
                if p.efscale.is_empty() {
                    "Surveyed"
                } else {
                    &p.efscale
                }
            ),
            body,
            color,
            image: key.clone(),
            link: None,
        });
        let (Some(key), Some(url)) = (key, p.image.clone()) else {
            return;
        };
        if self.pf_icon_tex.contains_key(&key) {
            return; // loaded, or already in flight
        }
        self.pf_icon_tex.insert(key.clone(), None);
        let http = self.http.clone();
        let tx = self.pf_icon_tx.clone();
        let ctx2 = ctx.clone();
        self.spawner.spawn(async move {
            match fetch_icon_sheet(&http, &url).await {
                Ok(image) => {
                    let _ = tx.send((key, image));
                    ctx2.request_repaint();
                }
                Err(e) => log::warn!("survey photo {url} failed: {e}"),
            }
        });
    }

    /// Drive the river-gauge fetch (NWPS), mirroring [`Self::sync_metar`] but with a slower cadence
    /// (gauge stages update every ~15 min upstream, so 300 s is plenty).
    pub(crate) fn sync_gauges(&mut self, ctx: &egui::Context) {
        if !self.show_gauges {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        // A view that has not been laid out yet has no finite bounds; asking with them returned
        // the entire national dataset from the service.
        if ![min_lon, min_lat, max_lon, max_lat]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        if (max_lon - min_lon) > GAUGE_MAX_SPAN_DEG {
            return; // too zoomed out — too many gauges to be readable
        }
        let (clon, clat) = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        let stale = self
            .gauge_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 300);
        let drifted = self.gauge_bounds.is_none_or(|(la0, lo0, la1, lo1)| {
            let (mlon, mlat) = ((lo0 + lo1) * 0.5, (la0 + la1) * 0.5);
            let (hw, hh) = ((lo1 - lo0) * 0.25, (la1 - la0) * 0.25);
            (clon - mlon).abs() > hw || (clat - mlat).abs() > hh
        });
        // The service allows ten requests per five minutes, and panning past a quarter of the
        // view counts as drifting, so a busy pan could exhaust that in seconds. Panning waits for
        // a settled view; only the ordinary refresh cadence ignores this.
        let settled = self
            .gauge_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 30);
        if stale || (drifted && settled) {
            let pad_lon = ((max_lon - min_lon) * 0.2).min(15.0);
            let pad_lat = ((max_lat - min_lat) * 0.2).min(15.0);
            let (lat0, lon0) = (min_lat - pad_lat, min_lon - pad_lon);
            let (lat1, lon1) = (max_lat + pad_lat, max_lon + pad_lon);
            self.gauge_last_fetch = Some(Instant::now());
            self.gauge_bounds = Some((lat0, lon0, lat1, lon1));
            self.spawn_overlay(ctx, OverlaySource::Gauges(lat0, lon0, lat1, lon1));
        }
    }

    /// Keep the GOES cloud-top-height field for the 3D map's satellite surface current: fetched
    /// only while some pane shows it, for the five-minute slot of the view's time (so it follows
    /// a scrubbed view, and refreshes live as the slot turns over).
    pub(crate) fn sync_cloud_top(&mut self) {
        if let Some((key, rx)) = &self.cloud_top_rx {
            match rx.try_recv() {
                Ok(field) => {
                    self.cloud_top = Some((*key, field));
                    self.cloud_top_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.cloud_top_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        let wanted = self
            .views
            .iter()
            .any(|v| v.map_3d.enabled && v.map_3d.cloud_top_surface);
        if !wanted {
            return;
        }
        let at = self.view_target_time();
        let west = self.settings.goes_satellite_west;
        let key = (
            at.unwrap_or_else(Utc::now).timestamp().div_euclid(300),
            west,
        );
        if self.cloud_top.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let satellite = if west {
            wxdata::goes_abi::Satellite::West
        } else {
            wxdata::goes_abi::Satellite::East
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.cloud_top_rx = Some((key, rx));
        let http = self.http.clone();
        self.spawner.spawn(async move {
            match wxdata::goes_abi::fetch_cloud_top_height(&http, satellite, at, 900, 525).await {
                Ok(field) => {
                    let _ = tx.send(field);
                }
                Err(e) => log::warn!("cloud top height: {e:#}"),
            }
        });
    }
}
