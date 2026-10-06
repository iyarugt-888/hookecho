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
                      its VCP, tilt and elevation, the detector algorithm versions, Tornado ID's\n\
                      lineage (pipeline, versions, input scan times) and the melting level in use.\n\
detections.csv        The debris signatures, rotation couplets and Tornado ID verdicts on the active\n\
                      pane's volume, as shown, with their score and algorithm version. Tornado ID\n\
                      rows say which pipeline made them and when their input sweeps were scanned\n\
                      (blank where that was not recorded).\n\
probes/*.csv          Whichever probes were open: region statistics, the gate inspector's vertical\n\
                      profile and time series, the cross-section.\n\
grid.tif              The top gridded layer on the active pane, as a float32 GeoTIFF (EPSG:4326,\n\
                      NaN = no data), when one is on.\n\
\n\
Radar data is public (NOAA NEXRAD Level II via the AWS Open Data programme) and is not included;\n\
provenance.json names every volume used, so it can be fetched again.\n";

/// Tornado ID verdicts as `detections.csv` rows: the score in the confidence column, the
/// pipeline's own version, and the input sweeps' scan interval (blank when not recorded).
fn tornado_csv_rows(
    ids: &[wxdata::tornado_id::TornadoId],
    lineage: &wxdata::detection_lineage::DetectionLineage,
) -> String {
    use wxdata::detection_lineage::Pipeline;
    let (pipeline, version) = match lineage.pipeline {
        Pipeline::Fused => ("fused", wxdata::tornado_fusion::ALGORITHM_VERSION),
        Pipeline::Original => ("original", wxdata::tornado_id::ALGORITHM_VERSION),
    };
    let utc = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|t| t.to_rfc3339())
            .unwrap_or_default()
    };
    let (start, end) = lineage
        .inputs
        .as_ref()
        .and_then(|c| c.acquisition_range_ms())
        .map_or((String::new(), String::new()), |(a, b)| (utc(a), utc(b)));
    ids.iter()
        .map(|t| {
            format!(
                "tornado_id,{:.4},{:.4},{:.3},,,{version},{},{pipeline},{start},{end}\n",
                t.lat,
                t.lon,
                t.score,
                t.tier.label().replace(' ', "_").to_ascii_lowercase()
            )
        })
        .collect()
}

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
        // A column user product's units are its own; every other layer's are in its long name.
        let units = (layer == crate::render::FieldLayer::UserColumn)
            .then(|| self.column_shown(self.active).map(|a| a.units.clone()))
            .flatten()
            .filter(|u| !u.is_empty());
        let nc = wxdata::netcdf::write(grid, &name, &product, units.as_deref(), &source)?;
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
            // Only a grid this pane is actually drawing: a local radar field built for another
            // pane's selection is not this pane's to export.
            if !view.fields_on.contains(layer)
                || !self.mrms_ready(*layer)
                || !self.radar_field_ready(self.active, *layer)
            {
                return None;
            }
            let state = self.field_state_for(self.active, *layer)?;
            let grid = state.grid.as_ref()?;
            let product =
                match self
                    .column_shown(self.active)
                    .filter(|_| *layer == FL::UserColumn)
                {
                    // The formula travels with the grid, and where any isotherm in it came from.
                    Some(a) => {
                        let formula = self
                            .settings
                            .udp_products
                            .iter()
                            .find(|p| p.name == a.name)
                            .map_or_else(String::new, |p| p.expression.clone());
                        let env = a
                            .env_source
                            .as_ref()
                            .map_or_else(String::new, |s| format!(" | environment {s}"));
                        format!(
                            "{} [{}] = {formula} (column user product, {} tilts){env}",
                            a.name, a.units, a.product.tilts
                        )
                    }
                    None if *layer == FL::UserColumnTrail => {
                        match self
                            .column_trail_shown(self.active)
                            .and_then(|t| Some((t, t.shown.as_ref()?)))
                        {
                            Some((t, w)) => {
                                format!(
                            "{} [{}] trail: {} over {} min ending at the playhead, {} volumes \
                             {}..{} ({} missing, {} s of the window without history)",
                            t.name,
                            t.units,
                            if self.filters.trail_keep_min { "minimum" } else { "maximum" },
                            self.filters.trail_window_min,
                            w.coverage.frames,
                            w.coverage.from,
                            w.coverage.to,
                            w.coverage.missing,
                            w.coverage.short_s
                        )
                            }
                            None => Self::probe_field_product(*layer),
                        }
                    }
                    None => Self::probe_field_product(*layer),
                };
            Some((
                *layer,
                grid,
                product,
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
            "tornado_id": self.tornado_lineage_json(),
            "melting_level": self.freezing.as_ref().map(|l| json!({
                "site": l.site,
                "source": if l.epoch.is_some() { "observed sounding" } else { "HRRR analysis" },
                "source_detail": l.source,
                "synoptic_time_utc": l.epoch.map(|t| t.to_rfc3339()),
                "zero_c_m_msl": l.h0_m,
                "minus10_c_m_msl": l.hm10_m,
                "minus10_c_crossings": l.hm10_crossings,
                "minus20_c_m_msl": l.hm20_m,
                "minus30_c_m_msl": l.hm30_m,
                "minus30_c_crossings": l.hm30_crossings,
                "minus40_c_m_msl": l.hm40_m,
                "minus40_c_crossings": l.hm40_crossings,
            })),
            "sources": [
                "NOAA NEXRAD Level II (AWS Open Data: unidata-nexrad-level2)",
                "Detections: HookEcho heuristics, not NWS products",
            ],
        })
    }

    /// The active pane's shown debris signatures and couplets as CSV rows.
    fn detections_csv(&self) -> String {
        let mut out = String::from(
            "kind,lat,lon,confidence,range_km,tilts,algorithm,tier,pipeline,inputs_start_utc,inputs_end_utc\n",
        );
        let Some(name) = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.name.clone())
        else {
            return out;
        };
        for h in self.tds_shown_cache.peek(&name).into_iter().flatten() {
            out.push_str(&format!(
                "debris,{:.4},{:.4},{:.3},{:.1},{},{},,,,\n",
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
                "rotation,{:.4},{:.4},{:.3},{:.1},{},{},,,,\n",
                c.lat,
                c.lon,
                c.confidence,
                c.range_km,
                c.tilts,
                wxdata::rotation::ALGORITHM_VERSION
            ));
        }
        if let Some((_, ids, lineage)) = self.tornado_shown_for(&name) {
            out.push_str(&tornado_csv_rows(ids, lineage));
        }
        out
    }

    /// The active pane's Tornado ID verdicts and their lineage, when they are of its volume.
    #[allow(clippy::type_complexity)]
    pub(crate) fn tornado_shown_for(
        &self,
        volume: &str,
    ) -> Option<&(
        String,
        Vec<wxdata::tornado_id::TornadoId>,
        wxdata::detection_lineage::DetectionLineage,
    )> {
        self.tornado_shown.as_ref().filter(|(v, _, _)| v == volume)
    }

    /// Tornado ID's lineage on the active pane's volume, or why there is none.
    pub(crate) fn tornado_lineage_json(&self) -> serde_json::Value {
        let name = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.name.clone())
            .unwrap_or_default();
        match self.tornado_shown_for(&name) {
            Some((_, ids, lineage)) => {
                let mut j = lineage.to_json();
                j["verdicts"] = serde_json::Value::from(ids.len());
                j
            }
            None => serde_json::json!({
                "shown": false,
                "note": "Tornado ID was not shown on this volume, so no verdicts or lineage were recorded",
            }),
        }
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
            imported: &[],
            tracks: &[],
            routes: &[],
            route_selected: 0,
            route_engine: "",
            contours: &[],
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

#[cfg(test)]
mod lineage_export_tests {
    use super::*;
    use wxdata::detection_lineage::{DetectionLineage, Pipeline};

    #[test]
    fn tornado_rows_name_their_pipeline_and_input_scan_times() {
        let id = wxdata::tornado_id::TornadoId {
            lon: -97.48,
            lat: 35.33,
            tier: wxdata::tornado_id::Tier::Debris,
            score: 0.82,
            terms: Vec::new(),
            vrot_ms: Some(40.0),
            min_cc: Some(0.6),
            reasons: Vec::new(),
        };
        let mut z = wxdata::level2::BinnedSweep {
            az_bins: 2,
            gate_count: 1,
            data: vec![10, 10],
            elevation_deg: 0.5,
            bin_time_ms: vec![1_369_080_749_000, 1_369_080_772_000],
            ..Default::default()
        };
        z.moment = wxdata::level2::Moment::Reflectivity;
        let lineage = DetectionLineage {
            pipeline: Pipeline::Fused,
            algorithms: wxdata::detection_lineage::fused_algorithms(),
            site: Some("KTLX".into()),
            volume: "KTLX20130520_201229_V06".into(),
            volume_time: None,
            inputs: wxdata::detection_lineage::input_coverage(vec![z]),
            stand_in: None,
        };
        let rows = tornado_csv_rows(std::slice::from_ref(&id), &lineage);
        assert_eq!(
            rows,
            "tornado_id,35.3300,-97.4800,0.820,,,fusion-3,tornado_debris,fused,\
             2013-05-20T20:12:29+00:00,2013-05-20T20:12:52+00:00\n"
        );
        // Not recorded: blank, never the volume's nominal time.
        let original = DetectionLineage {
            pipeline: Pipeline::Original,
            algorithms: wxdata::detection_lineage::original_algorithms(),
            inputs: None,
            ..lineage
        };
        let rows = tornado_csv_rows(&[id], &original);
        assert!(
            rows.ends_with(",tornado-id-1,tornado_debris,original,,\n"),
            "{rows}"
        );
    }
}
