//! Satellite-native playback (ROADMAP_PARITY M5.2): a loop through the satellite's own scans —
//! every one-minute mesoscale frame, or every five-minute CONUS frame — instead of the frame
//! nearest each radar volume. While it runs it drives the GOES layers' clock; radar and every
//! other layer keep their own.
//!
//! The frame list comes from the bucket's own listing (`wxdata::goes_abi::scan_frames`), so a
//! scan that never landed is a visible gap on the loop's strip rather than a closed-up step. The
//! loop steps only once the frame on screen is the one asked for (or after a bounded wait, when
//! that frame is marked failed), so playback never runs ahead into blank frames. A few frames
//! ahead are downloaded into the object cache (disk, or IndexedDB in the browser) without being
//! decoded, so memory holds one decoded frame per layer however long the loop is.
use super::*;
use wxdata::goes_abi::{ScanFrame, ScanGap, Sector};

/// Loop lengths offered, minutes.
pub(crate) const WINDOWS_MIN: [u16; 4] = [15, 30, 60, 120];
/// Playback speeds offered, frames per second (an upper bound: a frame not yet shown holds).
pub(crate) const SPEEDS_FPS: [f32; 3] = [2.0, 4.0, 8.0];
/// How long a frame may take to appear before the loop moves on and marks it failed.
const FRAME_WAIT_S: f64 = 12.0;
/// Pause on the newest frame before the loop starts over.
const END_DWELL_S: f64 = 1.0;
/// Frames downloaded ahead of the playhead, and at most this many at once.
const PREFETCH_AHEAD: usize = 6;
const PREFETCH_INFLIGHT: usize = 2;
/// A live loop relists this often, to pick up new scans.
const LIVE_RELIST_S: f64 = 30.0;

/// What a listing was made for. A change of any part relists and drops the old frames.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopKey {
    pub west: bool,
    pub sector: Sector,
    /// The band whose scans are the frames, then the other bands the shown layers read.
    pub bands: Vec<u8>,
    pub window_min: u16,
    /// The loop's end: `None` follows now (a live loop), `Some` an archive analysis time.
    pub end: Option<DateTime<Utc>>,
}

type Listing = (u64, Result<Vec<Vec<ScanFrame>>, String>);

#[derive(Default)]
pub(crate) struct SatLoop {
    pub on: bool,
    pub window_min: u16,
    pub fps: f32,
    pub playing: bool,
    /// The loop's frames: the first band's scans, oldest first.
    pub frames: Vec<ScanFrame>,
    /// Each band's scans (same order as `LoopKey::bands`), for prefetching the other bands.
    band_frames: Vec<Vec<ScanFrame>>,
    pub gaps: Vec<ScanGap>,
    pub playhead: usize,
    /// Scan starts that did not appear within the wait.
    pub failed: std::collections::HashSet<DateTime<Utc>>,
    pub key: Option<LoopKey>,
    pub error: Option<String>,
    pub listing: bool,
    generation: u64,
    listed_at: Option<f64>,
    rx: Option<std::sync::mpsc::Receiver<Listing>>,
    tx: Option<std::sync::mpsc::Sender<Listing>>,
    /// When the frame on the playhead was asked for (UI clock seconds).
    asked_at: Option<f64>,
    /// When the playhead last moved.
    stepped_at: Option<f64>,
    /// Prefetch bookkeeping, by object key.
    pub cached: std::collections::HashSet<String>,
    inflight: std::collections::HashSet<String>,
    prefetch_rx: Option<std::sync::mpsc::Receiver<(String, bool)>>,
    prefetch_tx: Option<std::sync::mpsc::Sender<(String, bool)>>,
}

impl SatLoop {
    pub(crate) fn new() -> Self {
        Self {
            window_min: 60,
            fps: 4.0,
            ..Default::default()
        }
    }

    /// The scan the GOES layers should show, while the loop is on and has frames.
    pub(crate) fn current(&self) -> Option<DateTime<Utc>> {
        self.on
            .then(|| self.frames.get(self.playhead).map(|f| f.start))
            .flatten()
    }

    /// Install a new listing, keeping the playhead on the same scan when it is still listed (or
    /// the nearest one), and on the newest when it was there or nothing was shown yet.
    pub(crate) fn install(&mut self, bands: Vec<Vec<ScanFrame>>, cadence_secs: u64) {
        let was = self.frames.get(self.playhead).map(|f| f.start);
        let at_head = self.playhead + 1 >= self.frames.len();
        self.frames = bands.first().cloned().unwrap_or_default();
        self.band_frames = bands;
        self.gaps = wxdata::goes_abi::scan_gaps(&self.frames, cadence_secs);
        self.playhead = match was {
            Some(t) if !at_head => self
                .frames
                .iter()
                .enumerate()
                .min_by_key(|(_, f)| (f.start - t).num_seconds().abs())
                .map_or(0, |(i, _)| i),
            _ => self.frames.len().saturating_sub(1),
        };
        self.asked_at = None;
    }

    /// Move the playhead to `i` (clamped), as a step or a seek.
    pub(crate) fn seek(&mut self, i: usize, now: f64) {
        if self.frames.is_empty() {
            return;
        }
        self.playhead = i.min(self.frames.len() - 1);
        self.asked_at = Some(now);
        self.stepped_at = Some(now);
    }

    pub(crate) fn step_by(&mut self, delta: i32, now: f64) {
        let n = self.frames.len() as i32;
        if n == 0 {
            return;
        }
        self.seek((self.playhead as i32 + delta).rem_euclid(n) as usize, now);
    }

    /// Advance playback at `now` (UI clock seconds). `shown` is whether the frame on the playhead
    /// is the one on screen. Returns whether the playhead moved.
    pub(crate) fn tick(&mut self, now: f64, shown: bool) -> bool {
        if !self.playing || self.frames.len() < 2 {
            return false;
        }
        let asked = *self.asked_at.get_or_insert(now);
        let current = self.frames[self.playhead].start;
        if !shown {
            if now - asked < FRAME_WAIT_S {
                return false;
            }
            // Never appeared: say so on the strip and move on rather than stall the loop.
            self.failed.insert(current);
        }
        let since = now - self.stepped_at.unwrap_or(f64::NEG_INFINITY);
        let dwell = 1.0 / self.fps.max(0.5) as f64
            + if self.playhead + 1 == self.frames.len() {
                END_DWELL_S
            } else {
                0.0
            };
        if since < dwell {
            return false;
        }
        self.step_by(1, now);
        true
    }

    /// The object keys to download next: the frames after the playhead (wrapping), each band's
    /// scan nearest that frame, not already cached or in flight, within the in-flight bound.
    pub(crate) fn prefetch_targets(&self) -> Vec<String> {
        let n = self.frames.len();
        if n == 0 {
            return Vec::new();
        }
        let room = PREFETCH_INFLIGHT.saturating_sub(self.inflight.len());
        let mut out = Vec::new();
        for k in 1..=PREFETCH_AHEAD.min(n - 1) {
            let t = self.frames[(self.playhead + k) % n].start;
            for band in &self.band_frames {
                let Some(f) = band
                    .iter()
                    .min_by_key(|f| (f.start - t).num_seconds().abs())
                    .filter(|f| (f.start - t).num_seconds().abs() <= 30)
                else {
                    continue;
                };
                if !self.cached.contains(&f.key)
                    && !self.inflight.contains(&f.key)
                    && !out.contains(&f.key)
                {
                    out.push(f.key.clone());
                }
            }
        }
        out.truncate(room);
        out
    }

    /// Frames whose every band is in the cache, for the strip's "cached" count.
    pub(crate) fn cached_frames(&self) -> usize {
        self.frames
            .iter()
            .filter(|f| self.cached.contains(&f.key))
            .count()
    }

    /// The loop's line: satellite, sector, frame time, position, gaps and cache.
    pub(crate) fn summary(&self) -> String {
        let mut s = match &self.key {
            Some(key) => format!(
                "{} {}",
                if key.west { "GOES-West" } else { "GOES-East" },
                key.sector.label()
            ),
            None => "Satellite loop".into(),
        };
        if let Some(f) = self.frames.get(self.playhead) {
            s.push_str(&format!(
                " \u{b7} {} UTC \u{b7} {}/{}",
                f.start.format("%H:%M:%S"),
                self.playhead + 1,
                self.frames.len()
            ));
        }
        let missing: u32 = self.gaps.iter().map(|g| g.missing).sum();
        if missing > 0 {
            s.push_str(&format!(
                " \u{b7} {missing} scan{} missing",
                if missing == 1 { "" } else { "s" }
            ));
        }
        if !self.failed.is_empty() {
            s.push_str(&format!(" \u{b7} {} failed to load", self.failed.len()));
        }
        s
    }
}

/// The ABI band a GOES layer reads; `None` for the composites and derived layers.
pub(crate) fn goes_layer_band(layer: crate::render::FieldLayer) -> Option<u8> {
    use crate::render::FieldLayer as FL;
    Some(match layer {
        FL::GoesIr | FL::GoesColdTop => 13,
        FL::GoesVisible => 2,
        FL::GoesWaterVapor => 8,
        FL::GoesShortwaveIr => 7,
        FL::GoesMidWaterVapor => 9,
        FL::GoesLowWaterVapor => 10,
        FL::GoesDirtyIr => 15,
        FL::GoesLongwaveIr => 14,
        _ => return None,
    })
}

impl HookEchoApp {
    /// The GOES frame layers the active pane shows.
    fn sat_loop_layers(&self) -> Vec<crate::render::FieldLayer> {
        GOES_FRAME_LAYERS
            .into_iter()
            .filter(|l| self.views[self.active].fields_on.contains(l))
            .collect()
    }

    /// What the loop would list now: the active pane's satellite, the chosen sector,
    /// the bands its GOES layers read (band 13 first when nothing else decides), and its end.
    fn sat_loop_key(&self) -> Option<LoopKey> {
        let layers = self.sat_loop_layers();
        if layers.is_empty() {
            return None;
        }
        let mut bands = Vec::new();
        for layer in &layers {
            if let Some(b) = goes_layer_band(*layer) {
                bands.push(b);
            } else if *layer == crate::render::FieldLayer::GoesRgb {
                bands.extend(
                    wxdata::goes_rgb::by_slug(&self.settings.goes_rgb_recipe)
                        .unwrap_or(&wxdata::goes_rgb::AIR_MASS)
                        .bands(),
                );
            }
        }
        if bands.is_empty() {
            bands.push(13);
        }
        let mut seen = std::collections::HashSet::new();
        bands.retain(|b| seen.insert(*b));
        // The radar's own clock decides where an archive loop ends; a live pane follows now.
        let end = if self.views[self.active].timeline.following {
            None
        } else {
            self.model_target_time(self.active)
        };
        Some(LoopKey {
            west: self.settings.goes_satellite_west,
            // The chosen sector, not the pane's fallback: a mesoscale box moving off the view as
            // the loop plays would otherwise flip the key and relist mid-loop.
            sector: self.settings.goes_sector,
            bands,
            window_min: self.sat_loop.window_min,
            end,
        })
    }

    pub(crate) fn toggle_sat_loop(&mut self) {
        let l = &mut self.sat_loop;
        l.on = !l.on;
        l.playing = l.on;
        if !l.on {
            // Back to the frame nearest the radar, as before the loop.
            l.key = None;
            l.frames.clear();
            l.band_frames.clear();
            l.gaps.clear();
            l.failed.clear();
            l.error = None;
            l.generation += 1;
        }
    }

    /// Per frame: relist on a new context (and periodically while live), take finished listings
    /// and prefetches, advance playback once the shown frame matches, and start prefetches.
    pub(crate) fn update_sat_loop(&mut self, ctx: &egui::Context) {
        if !self.sat_loop.on {
            return;
        }
        let now = ctx.input(|i| i.time);
        let key = self.sat_loop_key();
        if key.is_none() {
            self.sat_loop.error = Some("Turn on a GOES layer to loop it".into());
        }
        let relist = key.as_ref().is_some_and(|k| {
            self.sat_loop.key.as_ref() != Some(k)
                || (k.end.is_none()
                    && !self.sat_loop.listing
                    && self
                        .sat_loop
                        .listed_at
                        .is_none_or(|t| now - t >= LIVE_RELIST_S))
        });
        if let (true, Some(key)) = (relist, key) {
            self.start_sat_listing(key, now, ctx);
        }
        // Finished listings: only the newest generation's.
        if let Some(rx) = &self.sat_loop.rx {
            let mut got = None;
            while let Ok(msg) = rx.try_recv() {
                got = Some(msg);
            }
            if let Some((generation, result)) = got.filter(|(g, _)| *g == self.sat_loop.generation)
            {
                self.sat_loop.listing = false;
                match result {
                    Ok(bands) => {
                        let cadence = self
                            .sat_loop
                            .key
                            .as_ref()
                            .map_or(60, |k| k.sector.cadence_secs());
                        self.sat_loop.install(bands, cadence);
                        self.sat_loop.error = self
                            .sat_loop
                            .frames
                            .is_empty()
                            .then(|| "No scans listed in this window".to_string());
                    }
                    Err(e) => self.sat_loop.error = Some(e),
                }
                let _ = generation;
            }
        }
        if let Some(rx) = &self.sat_loop.prefetch_rx {
            while let Ok((key, ok)) = rx.try_recv() {
                self.sat_loop.inflight.remove(&key);
                if ok {
                    self.sat_loop.cached.insert(key);
                }
            }
        }
        // Shown: every GOES layer on the pane holds the scan the loop asked for.
        let shown = self.sat_loop.current().is_some_and(|_| {
            self.sat_loop_layers()
                .into_iter()
                .all(|l| self.goes_ready_for(self.active, l))
        });
        if self.sat_loop.tick(now, shown) || self.sat_loop.playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.start_sat_prefetch(ctx);
    }

    fn start_sat_listing(&mut self, key: LoopKey, now: f64, ctx: &egui::Context) {
        let l = &mut self.sat_loop;
        if l.key.as_ref() != Some(&key) {
            // Another context: its frames, gaps and failures are not this one's.
            l.frames.clear();
            l.band_frames.clear();
            l.gaps.clear();
            l.failed.clear();
            l.playhead = 0;
        }
        l.generation += 1;
        l.listing = true;
        l.listed_at = Some(now);
        l.key = Some(key.clone());
        if l.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            l.tx = Some(tx);
            l.rx = Some(rx);
        }
        let tx = l.tx.clone().expect("set above");
        let generation = l.generation;
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let to = key.end.unwrap_or_else(Utc::now);
            let from = to - chrono::Duration::minutes(key.window_min as i64);
            let satellite = if key.west {
                wxdata::goes_abi::Satellite::West
            } else {
                wxdata::goes_abi::Satellite::East
            };
            let mut bands = Vec::new();
            let mut result = Ok(());
            for band in &key.bands {
                match wxdata::goes_abi::scan_frames(&http, satellite, key.sector, *band, from, to)
                    .await
                {
                    Ok(f) => bands.push(f),
                    Err(e) => {
                        result = Err(format!("Scan listing failed: {e}"));
                        break;
                    }
                }
            }
            let _ = tx.send((generation, result.map(|()| bands)));
            ctx.request_repaint();
        });
    }

    fn start_sat_prefetch(&mut self, ctx: &egui::Context) {
        let Some(key) = self.sat_loop.key.clone() else {
            return;
        };
        let targets = self.sat_loop.prefetch_targets();
        if targets.is_empty() {
            return;
        }
        let l = &mut self.sat_loop;
        if l.prefetch_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            l.prefetch_tx = Some(tx);
            l.prefetch_rx = Some(rx);
        }
        let satellite = if key.west {
            wxdata::goes_abi::Satellite::West
        } else {
            wxdata::goes_abi::Satellite::East
        };
        for object in targets {
            l.inflight.insert(object.clone());
            let tx = l.prefetch_tx.clone().expect("set above");
            let http = self.http.clone();
            let ctx = ctx.clone();
            self.spawner.spawn(async move {
                let ok = wxdata::goes_abi::prefetch_key(&http, satellite, &object)
                    .await
                    .inspect_err(|e| log::debug!("satellite prefetch {object}: {e:#}"))
                    .is_ok();
                let _ = tx.send((object, ok));
                ctx.request_repaint();
            });
        }
    }

    /// The loop's bar: play/pause, step, a strip of every listed scan with gaps and failures
    /// marked (click or drag to seek), length and speed, and close.
    pub(crate) fn sat_loop_bar(&mut self, ctx: &egui::Context) {
        if !self.sat_loop.on {
            return;
        }
        let now = ctx.input(|i| i.time);
        let t = self.ws_tokens();
        let mut close = false;
        egui::Area::new(egui::Id::new("sat_loop_bar"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -72.0))
            .show(ctx, |ui| {
                crate::ui::workstation::card_frame(&t).show(ui, |ui| {
                    crate::ui::workstation::style_scope(ui, &t);
                    let l = &mut self.sat_loop;
                    ui.horizontal(|ui| {
                        let play = if l.playing { "\u{23f8}" } else { "\u{25b6}" };
                        if ui
                            .button(play)
                            .on_hover_text("Play or pause the satellite loop")
                            .clicked()
                        {
                            l.playing = !l.playing;
                        }
                        if ui
                            .button("\u{25c0}")
                            .on_hover_text("Previous scan")
                            .clicked()
                        {
                            l.playing = false;
                            l.step_by(-1, now);
                        }
                        if ui.button("\u{25b6}|").on_hover_text("Next scan").clicked() {
                            l.playing = false;
                            l.step_by(1, now);
                        }
                        egui::ComboBox::from_id_salt("sat_loop_window")
                            .selected_text(format!("{} min", l.window_min))
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                for m in WINDOWS_MIN {
                                    ui.selectable_value(&mut l.window_min, m, format!("{m} min"));
                                }
                            });
                        egui::ComboBox::from_id_salt("sat_loop_fps")
                            .selected_text(format!("{:.0} fps", l.fps))
                            .width(64.0)
                            .show_ui(ui, |ui| {
                                for f in SPEEDS_FPS {
                                    ui.selectable_value(&mut l.fps, f, format!("{f:.0} fps"));
                                }
                            })
                            .response
                            .on_hover_text(
                                "At most this fast: a frame that has not loaded yet holds the loop",
                            );
                        if ui
                            .button("\u{d7}")
                            .on_hover_text("Close the satellite loop")
                            .clicked()
                        {
                            close = true;
                        }
                    });
                    // The strip: one tick per listed scan, gaps in the warning colour, failed
                    // frames hollow, the playhead filled.
                    let width = ui.available_width().clamp(220.0, 520.0);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, 18.0),
                        egui::Sense::click_and_drag(),
                    );
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, t.field);
                    if let (Some(first), Some(last)) = (l.frames.first(), l.frames.last()) {
                        let span = (last.start - first.start).num_seconds().max(1) as f32;
                        let x_of = |when: DateTime<Utc>| {
                            rect.left()
                                + 4.0
                                + (rect.width() - 8.0) * (when - first.start).num_seconds() as f32
                                    / span
                        };
                        for g in &l.gaps {
                            let r = egui::Rect::from_x_y_ranges(
                                x_of(g.after)..=x_of(g.before),
                                rect.y_range(),
                            );
                            painter.rect_filled(r.shrink2(egui::vec2(1.0, 3.0)), 0.0, t.warn);
                        }
                        for (i, f) in l.frames.iter().enumerate() {
                            let x = x_of(f.start);
                            let color = if l.failed.contains(&f.start) {
                                t.danger
                            } else if l.cached.contains(&f.key) {
                                t.text
                            } else {
                                t.text_dim
                            };
                            let h = if i == l.playhead { 8.0 } else { 4.0 };
                            painter.line_segment(
                                [
                                    egui::pos2(x, rect.center().y - h),
                                    egui::pos2(x, rect.center().y + h),
                                ],
                                egui::Stroke::new(if i == l.playhead { 3.0 } else { 1.0 }, color),
                            );
                        }
                        if let Some(p) = resp.interact_pointer_pos() {
                            let frac =
                                ((p.x - rect.left() - 4.0) / (rect.width() - 8.0)).clamp(0.0, 1.0);
                            let target =
                                first.start + chrono::Duration::seconds((frac * span) as i64);
                            if let Some((i, _)) = l
                                .frames
                                .iter()
                                .enumerate()
                                .min_by_key(|(_, f)| (f.start - target).num_seconds().abs())
                            {
                                l.playing = false;
                                l.seek(i, now);
                            }
                        }
                    }
                    let line = if l.listing && l.frames.is_empty() {
                        "Listing scans\u{2026}".to_string()
                    } else {
                        let mut s = l.summary();
                        if !l.frames.is_empty() {
                            s.push_str(&format!(
                                " \u{b7} {}/{} cached",
                                l.cached_frames(),
                                l.frames.len()
                            ));
                        }
                        s
                    };
                    ui.label(egui::RichText::new(line).small());
                    if let Some(e) = &l.error {
                        ui.label(egui::RichText::new(e).small().color(t.warn));
                    }
                });
            });
        if close {
            self.toggle_sat_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn frames(minutes: &[i64]) -> Vec<ScanFrame> {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 19, 18, 0, 17).unwrap();
        minutes
            .iter()
            .map(|m| ScanFrame {
                start: t0 + chrono::Duration::minutes(*m),
                key: format!("k{m}"),
            })
            .collect()
    }

    fn looping(minutes: &[i64]) -> SatLoop {
        let mut l = SatLoop::new();
        l.on = true;
        l.playing = true;
        l.install(vec![frames(minutes)], 60);
        l
    }

    #[test]
    fn a_new_loop_starts_on_the_newest_scan_and_shows_its_gaps() {
        let l = looping(&[0, 1, 2, 5, 6]);
        assert_eq!(l.playhead, 4);
        assert_eq!(l.current(), Some(frames(&[6])[0].start));
        assert_eq!(l.gaps.len(), 1);
        assert!(l.summary().contains("2 scans missing"), "{}", l.summary());
    }

    #[test]
    fn playback_waits_for_the_frame_on_screen_and_keeps_its_pace() {
        let mut l = looping(&[0, 1, 2, 3]);
        l.seek(0, 0.0);
        // Not shown yet: holds.
        assert!(!l.tick(1.0, false));
        assert_eq!(l.playhead, 0);
        // Shown, but faster than 4 fps allows: holds.
        l.fps = 4.0;
        assert!(!l.tick(0.1, true));
        assert!(l.tick(0.3, true));
        assert_eq!(l.playhead, 1);
        // A frame that never appears is marked failed after the wait and the loop moves on.
        assert!(!l.tick(5.0, false));
        assert!(l.tick(0.3 + FRAME_WAIT_S + 0.1, false));
        assert!(l.failed.contains(&frames(&[1])[0].start));
        assert_eq!(l.playhead, 2);
        // The newest frame dwells before the loop starts over.
        l.seek(3, 100.0);
        assert!(!l.tick(100.5, true));
        assert!(l.tick(100.0 + 0.25 + END_DWELL_S + 0.01, true));
        assert_eq!(l.playhead, 0, "wraps to the oldest");
        // Paused: never moves.
        l.playing = false;
        assert!(!l.tick(1_000.0, true));
    }

    #[test]
    fn a_relisting_keeps_the_scan_being_looked_at() {
        let mut l = looping(&[0, 1, 2, 3]);
        l.seek(1, 0.0);
        // A live relist drops the oldest scan and adds a new one.
        l.install(vec![frames(&[1, 2, 3, 4])], 60);
        assert_eq!(l.current(), Some(frames(&[1])[0].start));
        // On the newest, it stays on the newest as new scans arrive.
        let mut l = looping(&[0, 1, 2]);
        l.install(vec![frames(&[0, 1, 2, 3])], 60);
        assert_eq!(l.playhead, 3);
    }

    #[test]
    fn prefetch_looks_ahead_within_its_bound_and_skips_what_is_held() {
        let mut l = looping(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        l.seek(0, 0.0);
        assert_eq!(
            l.prefetch_targets(),
            ["k1", "k2"],
            "two at a time, nearest first"
        );
        l.cached.insert("k1".into());
        l.inflight.insert("k2".into());
        assert_eq!(l.prefetch_targets(), ["k3"], "one slot left");
        // Another band's scans a few seconds off the frame's own are fetched with it.
        let other: Vec<ScanFrame> = frames(&[0, 1, 2, 3])
            .into_iter()
            .map(|f| ScanFrame {
                start: f.start + chrono::Duration::seconds(4),
                key: format!("b2-{}", f.key),
            })
            .collect();
        let mut l = SatLoop::new();
        l.install(vec![frames(&[0, 1, 2, 3]), other], 60);
        l.seek(0, 0.0);
        assert_eq!(l.prefetch_targets(), ["k1", "b2-k1"]);
        assert_eq!(
            goes_layer_band(crate::render::FieldLayer::GoesVisible),
            Some(2)
        );
        assert_eq!(goes_layer_band(crate::render::FieldLayer::GoesRgb), None);
    }
}
