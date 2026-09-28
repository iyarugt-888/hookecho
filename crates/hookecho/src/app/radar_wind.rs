//! Radar winds: the wind particles driven by the active radar's own Doppler velocity instead of
//! the HRRR ([`wxdata::doppler_wind`]: each gate's measured radial speed, and the crosswise part
//! from the sweep's VAD). Rebuilt from the lowest dealiased velocity tilt whenever the displayed
//! scan changes.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;
use crate::wind_draw::WindField;
use wxdata::level2::Moment;

/// Grid cells per side, over at most [`MAX_HALF_KM`] each way from the radar: 1.25 km cells.
const GRID: usize = 240;
const MAX_HALF_KM: f64 = 150.0;

#[derive(Default)]
pub(crate) struct RadarWindState {
    pub(crate) on: bool,
    /// The scan (name, live revision) the field was last built from.
    key: Option<(String, u64)>,
    rx: Option<std::sync::mpsc::Receiver<Option<WindField>>>,
}

impl HookEchoApp {
    /// While radar winds are on, keep the particle field built from the active pane's scan.
    pub(crate) fn sync_radar_wind(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.radar_wind.rx {
            match rx.try_recv() {
                Ok(field) => {
                    self.radar_wind.rx = None;
                    if self.radar_wind.on {
                        if let Some(f) = field {
                            self.wind = Some(f);
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.radar_wind.rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        if !self.radar_wind.on || !self.show_wind {
            return;
        }
        let Some(key) = self.shown_volume_key(self.active) else {
            return;
        };
        if self.radar_wind.key.as_ref() == Some(&key) {
            return;
        }
        let Some(vol) = self.views[self.active].volume.as_mut() else {
            return;
        };
        let time = vol.time;
        let Ok(sweep) = vol.binned(Moment::Velocity, 0, true).cloned() else {
            return;
        };
        self.radar_wind.key = Some(key);
        let (tx, rx) = std::sync::mpsc::channel();
        self.radar_wind.rx = Some(rx);
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let built = wxdata::task::blocking(move || {
                let reach = sweep.first_gate_km as f64
                    + sweep.gate_count as f64 * sweep.gate_interval_km as f64;
                let half = reach.min(MAX_HALF_KM);
                let (u, v) = wxdata::doppler_wind::wind_field(&sweep, half, GRID, time)?;
                Some(WindField {
                    u,
                    v,
                    level: wxdata::hrrr::WindLevel::Surface,
                    run: time,
                    fcst_hour: 0,
                })
            })
            .await
            .ok()
            .flatten();
            let _ = tx.send(built);
            ctx.request_repaint();
        });
    }

    /// Radar winds switched: the particles need a new field from the other source.
    pub(crate) fn radar_wind_toggled(&mut self) {
        self.wind = None;
        self.wind_fetched = None;
        self.radar_wind.key = None;
        if self.radar_wind.on {
            self.show_wind = true;
        }
    }
}
