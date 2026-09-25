//! The analysis export (ROADMAP_NEW K4): one ZIP holding what an external tool needs to pick the
//! analysis up — the map as a PNG, the case manifest (panes, time window, bookmarks, product
//! definitions), the annotations as GeoJSON, provenance for every pane, the detections on the
//! active volume, and whichever probes are open (region statistics, the gate inspector's profile
//! and time series, the cross-section) as CSV. Not a word processor: every file is a plain format
//! something else already reads.
//!
//! The screenshot is asynchronous (a viewport command, answered by an event a frame later), so
//! "Export analysis…" requests one with [`super::ShotDest::Report`] and the archive is assembled
//! when the image arrives, from the state as it is then.

use super::{HookEchoApp, ToastKind};
use serde_json::json;

/// What the README says is in the archive, file by file.
const README: &str = "HookEcho analysis export\n\
\n\
map.png               The map as it was on screen.\n\
case.hookecho.json    The case: every pane's radar, product, tilt, camera and layers, the\n\
                      analysis time and replay window, bookmarks, markers, zones, drawings and\n\
                      user-defined products. Open it in HookEcho with Share > Open case.\n\
annotations.geojson   Drawings, markers and watch zones, for QGIS/ArcGIS or any GeoJSON reader.\n\
provenance.json       Which radar volume each pane showed (the NOAA object name and scan time),\n\
                      its VCP, tilt and elevation, the detector algorithm versions and the melting\n\
                      level in use.\n\
detections.csv        The debris signatures and rotation couplets on the active pane's volume, as\n\
                      shown (after corroboration), with their confidence and algorithm version.\n\
probes/*.csv          Whichever probes were open: region statistics, the gate inspector's vertical\n\
                      profile and time series, the cross-section.\n\
grid.tif              The top gridded layer on the active pane, as a float32 GeoTIFF (EPSG:4326,\n\
                      NaN = no data), when one is on.\n\
\n\
Radar data is public (NOAA NEXRAD Level II via the AWS Open Data programme) and is not included;\n\
provenance.json names every volume used, so it can be fetched again.\n";

impl HookEchoApp {
    /// The active pane's topmost visible gridded layer that holds a grid, as a GeoTIFF with its
    /// slug for a file name — the layer the cursor probe would read first, in draw order. The
    /// comparison layers (difference, A/B, ensemble) keep their grids elsewhere and are skipped.
    fn top_grid_geotiff(&self) -> Option<(&'static str, Vec<u8>)> {
        let (layer, grid, product, source, time) = self.top_grid()?;
        let description = format!(
            "{product} | {source} | valid {} | values as delivered by the source",
            time.format("%Y-%m-%dT%H:%MZ")
        );
        Some((layer.slug(), wxdata::geotiff::write(grid, &description)?))
    }

    /// The same grid as [`Self::top_grid_geotiff`], as CF NetCDF.
    fn top_grid_netcdf(&self) -> Option<(&'static str, Vec<u8>)> {
        let (layer, grid, product, source, time) = self.top_grid()?;
        // The file's time is the grid's; the layer's stamp is the better valid time when it has one.
        let grid = &wxdata::mrms::MrmsField {
            time,
            ..grid.clone()
        };
        // A NetCDF variable name is letters, digits and underscores; the slug has dashes.
        let name = layer.slug().replace('-', "_");
        let nc = wxdata::netcdf::write(grid, &name, &product, None, &source)?;
        Some((layer.slug(), nc))
    }

    /// The active pane's topmost visible layer that holds a grid, with its product and source
    /// names and valid time (the layer's stamp when it has one) — the one both grid exports write.
    fn top_grid(
        &self,
    ) -> Option<(
        crate::render::FieldLayer,
        &wxdata::mrms::MrmsField,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
    )> {
        use crate::render::FieldLayer as FL;
        let view = &self.views[self.active];
        FL::DRAW_ORDER.iter().rev().find_map(|layer| {
            if !view.fields_on.contains(layer) {
                return None;
            }
            let state = self.fields.get(layer)?;
            let grid = state.grid.as_ref()?;
            Some((
                *layer,
                grid,
                Self::probe_field_product(*layer),
                self.probe_field_source(*layer, Some(state)),
                state
                    .stamp
                    .as_ref()
                    .map_or(grid.time, |stamp| stamp.valid_time),
            ))
        })
    }

    /// Save the active pane's volume as CF/Radial 1.4 (ROADMAP_NEW M5): every tilt of every
    /// moment it carries, binned as displayed, velocity dealiased as the detectors read it.
    pub(crate) fn export_cfradial(&mut self) {
        let idx = self.active;
        let site_id = self.views[idx].site.clone();
        let Some(vol) = self.views[idx].volume.as_mut() else {
            self.toast(ToastKind::Error, "No radar volume on this pane to export");
            return;
        };
        let time = vol.time;
        let tilts: Vec<Vec<wxdata::level2::BinnedSweep>> = (0..vol.elevations.len())
            .map(|tilt| {
                wxdata::level2::Moment::ALL
                    .into_iter()
                    .filter_map(|m| {
                        vol.binned(m, tilt, m == wxdata::level2::Moment::Velocity)
                            .ok()
                            .cloned()
                    })
                    .collect()
            })
            .collect();
        let Some(s) = site_id.as_deref().and_then(wxdata::sites::site_by_id) else {
            self.toast(ToastKind::Error, "This pane's radar site is not known");
            return;
        };
        let site = wxdata::cfradial::Site {
            id: s.id,
            lat: s.latitude as f64,
            lon: s.longitude as f64,
            altitude_m: s.elevation_meters as f64 + wxdata::towers::tower_m(s.id),
        };
        let Some(nc) = wxdata::cfradial::write(site, time, &tilts) else {
            self.toast(ToastKind::Error, "Nothing in this volume to export");
            return;
        };
        let file = format!("{}_{}.nc", s.id, time.format("%Y%m%d_%H%M%S"));
        match crate::dialog::save_bytes(&file, "nc", &nc) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Volume saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("CF/Radial export failed: {e}");
                self.toast(ToastKind::Error, format!("CF/Radial export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Save the active pane's top gridded layer as CF NetCDF (ROADMAP_NEW M5).
    pub(crate) fn export_netcdf(&mut self) {
        let Some((slug, nc)) = self.top_grid_netcdf() else {
            self.toast(
                ToastKind::Error,
                "No gridded layer on this pane \u{2014} turn on MRMS, a derived or a model field",
            );
            return;
        };
        match crate::dialog::save_bytes(&format!("{slug}.nc"), "nc", &nc) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Grid saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("NetCDF export failed: {e}");
                self.toast(ToastKind::Error, format!("NetCDF export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Save the active pane's top gridded layer as a GeoTIFF (ROADMAP_NEW M5).
    pub(crate) fn export_geotiff(&mut self) {
        let Some((slug, tif)) = self.top_grid_geotiff() else {
            self.toast(
                ToastKind::Error,
                "No gridded layer on this pane \u{2014} turn on MRMS, a derived or a model field",
            );
            return;
        };
        match crate::dialog::save_bytes(&format!("{slug}.tif"), "tif", &tif) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Grid saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("GeoTIFF export failed: {e}");
                self.toast(ToastKind::Error, format!("GeoTIFF export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Start an analysis export: capture the map, then build the archive when the image lands.
    pub(crate) fn export_analysis(&mut self, ctx: &egui::Context) {
        self.request_capture(ctx, super::ShotDest::Report);
    }

    /// Every pane's data provenance, plus what the detectors and hail algorithm were running on.
    fn provenance(&self) -> serde_json::Value {
        let panes: Vec<serde_json::Value> = self
            .views
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let vol = v.volume.as_ref();
                json!({
                    "pane": i + 1,
                    "site": v.site,
                    "radar": v.site.as_deref().and_then(wxdata::sites::site_by_id).map(|s| json!({
                        "lat": s.latitude,
                        "lon": s.longitude,
                        "elevation_m": s.elevation_meters,
                    })),
                    "product": v.moment.short_name(),
                    "tilt_index": v.tilt,
                    "elevation_deg": vol.and_then(|vol| vol.elevations.get(v.tilt).copied()),
                    "volume": vol.map(|vol| vol.name.clone()),
                    "volume_time_utc": vol.map(|vol| vol.time.to_rfc3339()),
                    "vcp": vol.map(|vol| vol.vcp.clone()),
                    "following_live": v.timeline.following,
                })
            })
            .collect();
        json!({
            "app": "HookEcho",
            "app_version": env!("CARGO_PKG_VERSION"),
            "created_utc": chrono::Utc::now().to_rfc3339(),
            "panes": panes,
            "algorithms": {
                "debris_signature": wxdata::tds::ALGORITHM_VERSION,
                "rotation": wxdata::rotation::ALGORITHM_VERSION,
                "debris_min_confidence": self.settings.detectors.tds_min_confidence,
                "rotation_min_confidence": self.settings.detectors.rotation_min_confidence,
            },
            "melting_level": self.freezing.as_ref().map(|(site, epoch, h0, hm20)| json!({
                "site": site,
                "source": if epoch.is_some() { "observed sounding" } else { "HRRR analysis" },
                "synoptic_time_utc": epoch.map(|t| t.to_rfc3339()),
                "zero_c_m_msl": h0,
                "minus20_c_m_msl": hm20,
            })),
            "sources": [
                "NOAA NEXRAD Level II (AWS Open Data: unidata-nexrad-level2)",
                "Detections: HookEcho heuristics, not NWS products",
            ],
        })
    }

    /// The active pane's shown debris signatures and couplets as CSV rows.
    fn detections_csv(&self) -> String {
        let mut out = String::from("kind,lat,lon,confidence,range_km,tilts,algorithm\n");
        let Some(name) = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.name.clone())
        else {
            return out;
        };
        for h in self.tds_shown_cache.peek(&name).into_iter().flatten() {
            out.push_str(&format!(
                "debris,{:.4},{:.4},{:.3},{:.1},{},{}\n",
                h.lat,
                h.lon,
                h.confidence,
                h.range_km,
                h.tilts,
                wxdata::tds::ALGORITHM_VERSION
            ));
        }
        for c in self.rot_shown_cache.peek(&name).into_iter().flatten() {
            out.push_str(&format!(
                "rotation,{:.4},{:.4},{:.3},{:.1},{},{}\n",
                c.lat,
                c.lon,
                c.confidence,
                c.range_km,
                c.tilts,
                wxdata::rotation::ALGORITHM_VERSION
            ));
        }
        out
    }

    /// Assemble and save the archive, `image` being the map just captured.
    pub(crate) fn write_report(&mut self, image: &egui::ColorImage) {
        let now = chrono::Utc::now();
        let mut entries: Vec<(String, Vec<u8>)> = vec![("README.txt".into(), README.into())];
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let mut png = Vec::new();
        match image::RgbaImage::from_raw(w, h, rgba)
            .map(|img| img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png))
        {
            Some(Ok(())) => entries.push(("map.png".into(), png)),
            _ => log::warn!("analysis export: the map image could not be encoded"),
        }
        let case = self.capture_case();
        let name = case.name.clone();
        entries.push(("case.hookecho.json".into(), case.to_json().into_bytes()));
        let annotations = crate::gis_export::to_features(&crate::gis_export::MapContents {
            strokes: &self.strokes,
            markers: &self.settings.markers,
            zones: &self.settings.alert_polygons,
            cells: &[],
            overlays: &[],
        });
        entries.push((
            "annotations.geojson".into(),
            wxdata::gis::to_geojson(&annotations).into_bytes(),
        ));
        entries.push((
            "provenance.json".into(),
            serde_json::to_string_pretty(&self.provenance())
                .unwrap_or_default()
                .into_bytes(),
        ));
        entries.push(("detections.csv".into(), self.detections_csv().into_bytes()));
        if let Some(s) = self.region.samples() {
            entries.push(("probes/region.csv".into(), s.to_csv().into_bytes()));
        }
        if let Some(p) = &self.gate_popup {
            if p.column_inputs.len() >= 2 {
                entries.push((
                    "probes/profile.csv".into(),
                    crate::ui::gate_inspector::profile_csv(&p.column_inputs).into_bytes(),
                ));
            }
            if p.series.len() >= 2 {
                entries.push((
                    "probes/point-series.csv".into(),
                    crate::ui::gate_inspector::series_csv(p.moment, &p.series).into_bytes(),
                ));
            }
        }
        if let Some(xs) = &self.xsection {
            entries.push(("probes/xsection.csv".into(), xs.to_csv().into_bytes()));
        }
        if let Some((_, tif)) = self.top_grid_geotiff() {
            entries.push(("grid.tif".into(), tif));
        }
        let zip = crate::zipwrite::zip(&entries, now);
        let file = case.file_name().replace(".hookecho.json", "-analysis.zip");
        match crate::dialog::save_bytes(&file, "zip", &zip) {
            crate::dialog::Saved::Where(w) => self.toast(
                ToastKind::Success,
                format!("Analysis of {name} ({} files) saved to {w}", entries.len()),
            ),
            crate::dialog::Saved::Failed(e) => {
                log::warn!("analysis export failed: {e}");
                self.toast(ToastKind::Error, format!("Analysis export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }
}
