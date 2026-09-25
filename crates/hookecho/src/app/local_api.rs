//! The app's side of the local API (ROADMAP_NEW M4; the server is `crate::local_api`): start and
//! stop it from settings, publish what the app is showing about once a second (at once when the
//! displayed volume changes, which is what an event stream reports), and answer the questions the
//! server forwards — a sample at a point, a screenshot.

use super::{HookEchoApp, ShotDest, ToastKind};
use crate::local_api::{Handle, Request, Snapshot};
use serde_json::json;
use std::time::{Duration, Instant};

/// The server, and what the app last told it.
#[derive(Default)]
pub(crate) struct LocalApiState {
    handle: Option<Handle>,
    /// The port a start was refused on (in use, most likely), so it is not retried every frame;
    /// changing the port or turning the API off and on again tries once more.
    failed_port: Option<u16>,
    published: Option<Instant>,
    /// What the event stream counts as a change: the active pane's volume, tilt and product.
    shown: Option<String>,
}

/// How often the snapshot is refreshed when nothing on display has changed.
const REFRESH: Duration = Duration::from_secs(1);

impl HookEchoApp {
    /// Keep the server matching the settings, keep its snapshot fresh, and answer what it
    /// forwarded. Once a frame; a no-op with the API off.
    pub(crate) fn tick_local_api(&mut self, ctx: &egui::Context) {
        let want = self
            .settings
            .local_api
            .then_some(self.settings.local_api_port);
        let running = self.local_api.handle.as_ref().map(|h| h.port);
        if want.is_none() {
            self.local_api.handle = None;
            self.local_api.failed_port = None;
            return;
        }
        if running.is_some() && running != want {
            self.local_api.handle = None; // the port changed: restart on it below
        }
        let port = want.unwrap_or_default();
        if self.local_api.handle.is_none() && self.local_api.failed_port != Some(port) {
            let wake = ctx.clone();
            match crate::local_api::start(port, Box::new(move || wake.request_repaint())) {
                Ok(h) => {
                    log::info!("local API on http://127.0.0.1:{}/api/v1", h.port);
                    self.local_api.handle = Some(h);
                    self.local_api.published = None;
                }
                Err(e) => {
                    self.local_api.failed_port = Some(port);
                    self.toast(
                        ToastKind::Error,
                        format!("Local API could not use port {port}: {e}"),
                    );
                    return;
                }
            }
        }
        let shown = self.local_api_shown();
        let changed = shown != self.local_api.shown;
        if changed
            || self
                .local_api
                .published
                .is_none_or(|t| t.elapsed() >= REFRESH)
        {
            let snapshot = self.local_api_snapshot();
            if let Some(h) = &self.local_api.handle {
                h.publish(snapshot, changed);
            }
            self.local_api.published = Some(Instant::now());
            self.local_api.shown = shown;
        }
        let requests: Vec<Request> = self
            .local_api
            .handle
            .as_ref()
            .map(|h| h.requests.try_iter().collect())
            .unwrap_or_default();
        for request in requests {
            match request {
                Request::Sample { lat, lon, reply } => {
                    let _ = reply.send(self.local_api_sample(lat, lon));
                }
                Request::Snapshot { reply } => self.request_capture(ctx, ShotDest::Api(reply)),
            }
        }
    }

    /// The active pane's volume, tilt and product, as one key.
    fn local_api_shown(&self) -> Option<String> {
        let v = &self.views[self.active];
        let vol = v.volume.as_ref()?;
        Some(format!("{}|{}|{:?}", vol.name, v.tilt, v.moment))
    }

    fn local_api_snapshot(&mut self) -> Snapshot {
        let panes: Vec<_> = self
            .views
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let vol = v.volume.as_ref();
                let (lon, lat) =
                    crate::render::mercator::world_to_lonlat(v.camera.center.0, v.camera.center.1);
                json!({
                    "pane": i + 1,
                    "active": i == self.active,
                    "site": v.site,
                    "product": v.moment.short_name(),
                    "tilt_index": v.tilt,
                    "elevation_deg": vol.and_then(|vol| vol.elevations.get(v.tilt).copied()),
                    "volume": vol.map(|vol| vol.name.clone()),
                    "volume_time_utc": vol.map(|vol| vol.time.to_rfc3339()),
                    "following_live": v.timeline.following,
                    "camera": { "lon": lon, "lat": lat, "zoom": v.camera.zoom },
                })
            })
            .collect();
        let state = json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "time_utc": chrono::Utc::now().to_rfc3339(),
            "active_pane": self.active + 1,
            "panes": panes,
        });

        let v = &self.views[self.active];
        let name = v.volume.as_ref().map(|vol| vol.name.clone());
        let debris: Vec<_> = name
            .as_ref()
            .and_then(|n| self.tds_shown_cache.peek(n))
            .into_iter()
            .flatten()
            .map(|h| {
                json!({
                    "lat": h.lat, "lon": h.lon, "confidence": h.confidence,
                    "range_km": h.range_km, "tilts": h.tilts,
                    "rotation_kt": h.rotation_ms.map(|v| v * 1.943_844),
                })
            })
            .collect();
        let rotation: Vec<_> = name
            .as_ref()
            .and_then(|n| self.rot_shown_cache.peek(n))
            .into_iter()
            .flatten()
            .map(|c| {
                json!({
                    "lat": c.lat, "lon": c.lon, "confidence": c.confidence,
                    "range_km": c.range_km, "tilts": c.tilts,
                    "vrot_kt": c.vrot_ms * 1.943_844, "sense": c.sense.label(),
                })
            })
            .collect();
        let detections = json!({
            "volume": name,
            "debris": debris,
            "rotation": rotation,
            "algorithms": {
                "debris": wxdata::tds::ALGORITHM_VERSION,
                "rotation": wxdata::rotation::ALGORITHM_VERSION,
            },
            "note": "HookEcho heuristics, not NWS products",
        });

        let warnings: Vec<_> = self
            .active_alert_features()
            .iter()
            .filter_map(|f| f.alert.as_ref())
            .map(|a| {
                json!({
                    "event": a.event, "headline": a.headline, "area": a.area,
                    "expires_utc": a.expires.map(|t| t.to_rfc3339()),
                    "vtec": a.vtec, "tornado_detection": a.tornado_detection,
                })
            })
            .collect();

        let health = self.diagnostics_source_health();

        let v = &self.views[self.active];
        let products = json!({
            "site": v.site,
            "moments": v.volume.as_ref().map(|vol| {
                wxdata::level2::Moment::ALL
                    .iter()
                    .zip(vol.moments)
                    .filter(|(_, on)| *on)
                    .map(|(m, _)| m.short_name())
                    .collect::<Vec<_>>()
            }),
            "elevations_deg": v.volume.as_ref().map(|vol| vol.elevations.clone()),
            "field_layers_on": v.fields_on.iter().map(|l| l.slug()).collect::<Vec<_>>(),
        });
        let frames = json!({
            "site": v.site,
            "playhead": v.timeline.playhead,
            "following_live": v.timeline.following,
            "frames_utc": v
                .timeline
                .frames
                .iter()
                .filter_map(|id| id.date_time())
                .map(|t| t.to_rfc3339())
                .collect::<Vec<_>>(),
        });
        Snapshot {
            state: state.to_string(),
            detections: detections.to_string(),
            warnings: json!({ "warnings": warnings }).to_string(),
            health: serde_json::to_string(&json!({ "sources": health })).unwrap_or_default(),
            products: products.to_string(),
            frames: frames.to_string(),
        }
    }

    /// Every moment of the active pane's displayed tilt at `(lat, lon)`, velocity dealiased.
    fn local_api_sample(&mut self, lat: f64, lon: f64) -> String {
        let idx = self.active;
        let v = &mut self.views[idx];
        let (tilt, site) = (v.tilt, v.site.clone());
        let Some(vol) = v.volume.as_mut() else {
            return json!({ "error": "no radar volume on the active pane" }).to_string();
        };
        let mut values = serde_json::Map::new();
        let mut geometry = None;
        for m in wxdata::level2::Moment::ALL {
            let Ok(sweep) = vol.binned(m, tilt, m == wxdata::level2::Moment::Velocity) else {
                continue;
            };
            let sample = sweep.sample_at(lon, lat);
            if geometry.is_none() {
                geometry = sample.as_ref().map(|s| {
                    json!({
                        "azimuth_deg": s.azimuth_deg,
                        "slant_range_km": s.range_km,
                        "beam_height_ft": sweep.beam_height_ft(s.range_km),
                    })
                });
            }
            values.insert(
                m.short_name().to_string(),
                json!(sample.and_then(|s| s.value)),
            );
        }
        json!({
            "site": site,
            "volume": vol.name,
            "volume_time_utc": vol.time.to_rfc3339(),
            "elevation_deg": vol.elevations.get(tilt),
            "lat": lat,
            "lon": lon,
            "geometry": geometry,
            "values": values,
        })
        .to_string()
    }

    /// A captured window, as the PNG the API's snapshot endpoint waits for.
    pub(crate) fn answer_api_snapshot(
        reply: &std::sync::mpsc::Sender<Result<Vec<u8>, String>>,
        image: &egui::ColorImage,
    ) {
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let mut png = Vec::new();
        let result = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| "the captured image had the wrong size".to_string())
            .and_then(|img| {
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .map_err(|e| e.to_string())
            })
            .map(|()| png);
        let _ = reply.send(result);
    }
}
