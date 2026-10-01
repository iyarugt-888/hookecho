//! Loop export capture (GIF/MP4): step the timeline through every frame, wait for each to be
//! on screen, screenshot it, and encode. Moved out of `app.rs` (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Start a loop export (GIF or MP4): rewind the active timeline and capture every frame.
    pub(crate) fn start_loop_export(&mut self, format: crate::loopexport::LoopFormat) {
        use crate::loopexport::LoopFormat;
        let (name, ext) = match format {
            LoopFormat::Gif => ("hookecho-loop.gif", "gif"),
            LoopFormat::Mp4 => ("hookecho-loop.mp4", "mp4"),
        };
        let Some(path) = crate::dialog::save_path(name, ext) else {
            return;
        };
        let v = &mut self.views[self.active];
        let slots = v.timeline.frames.len(); // observed frames only (skip forecast tail)
        if slots == 0 {
            log::warn!("loop export: no timeline frames");
            self.toast(
                ToastKind::Info,
                "Nothing to export — no frames in the timeline yet",
            );
            return;
        }
        let speed = v.timeline.speed;
        v.timeline.go_begin();
        self.loop_export = Some(LoopExport {
            dest: path,
            format,
            frames: Vec::with_capacity(slots),
            remaining: slots,
            settle: LOOP_SETTLE_FRAMES,
            step_at: Instant::now(),
            capturing: false,
            fps: speed,
            volumes: Vec::with_capacity(slots),
            real_timing: self.settings.loop_real_timing,
        });
    }

    /// Advance the loop export: wait for the stepped radar to settle, then request a screenshot.
    pub(crate) fn drive_loop_export(&mut self, ctx: &egui::Context) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        if le.capturing {
            return; // waiting for the screenshot event
        }
        // The frame the timeline names must be the one on screen before anything counts: a
        // fixed number of repaints let a slow load be captured as the previous scan again (a
        // duplicated weather frame, filed under the wrong time). Only then do the settle frames
        // run, for the paint itself. A scan that never arrives is captured as whatever is shown
        // after LOOP_FRAME_TIMEOUT, and recorded as that (`record_loop_frame`), never as the
        // frame that was asked for.
        let v = &self.views[self.active];
        let wanted = v.timeline.current().map(|id| id.name().to_string());
        let shown = v.volume.as_ref().map(|x| x.name.as_str());
        if !frame_on_screen(shown, wanted.as_deref(), v.loading) {
            if le.step_at.elapsed() < LOOP_FRAME_TIMEOUT {
                ctx.request_repaint();
                return;
            }
            if le.settle == LOOP_SETTLE_FRAMES {
                log::warn!(
                    "loop export: {} not on screen after {}s; capturing {} instead",
                    wanted.as_deref().unwrap_or("?"),
                    LOOP_FRAME_TIMEOUT.as_secs(),
                    shown.unwrap_or("nothing"),
                );
            }
        }
        if le.settle > 0 {
            le.settle -= 1;
            ctx.request_repaint();
            return;
        }
        le.capturing = true;
        self.screenshot_pending = Some(ShotDest::Loop);
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    /// Record one captured loop frame; step to the next, or finish + encode the GIF.
    pub(crate) fn record_loop_frame(&mut self, image: &egui::ColorImage) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for px in &image.pixels {
            buf.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        if let Some(img) = image::RgbaImage::from_raw(w, h, buf) {
            le.frames.push(img);
            // What is on screen, not what the cursor names: the label must match the picture.
            le.volumes.push(
                self.views[self.active]
                    .volume
                    .as_ref()
                    .map(|v| (v.name.clone(), v.time)),
            );
        }
        le.capturing = false;
        le.remaining -= 1;
        if le.remaining > 0 {
            self.views[self.active].timeline.step(1);
            if let Some(le) = &mut self.loop_export {
                le.settle = LOOP_SETTLE_FRAMES;
                le.step_at = Instant::now();
            }
        } else {
            let le = self.loop_export.take().unwrap();
            use crate::loopexport::{LoopFormat, Timing};
            // Real timing needs every frame's scan time; a frame without one (the forecast tail)
            // puts the whole loop on the fixed rate rather than guessing.
            let volumes: Option<Vec<(String, DateTime<Utc>)>> =
                le.volumes.iter().cloned().collect();
            let fps = le.fps.clamp(1.0, 15.0);
            let timing = match (&volumes, le.real_timing) {
                (Some(_), true) => Timing::Real { fps },
                _ => Timing::Fixed { fps },
            };
            let frames = volumes
                .as_deref()
                .map(|v| crate::loopexport::frame_list(v, timing));
            let delays: Vec<u32> = match &frames {
                Some(f) => f.iter().map(|f| f.delay_ms).collect(),
                None => vec![(1000.0 / fps) as u32; le.frames.len()],
            };
            let res = match le.format {
                #[cfg(not(target_arch = "wasm32"))]
                LoopFormat::Gif => crate::loopexport::encode_gif_timed(
                    le.frames.iter().cloned().map(Ok),
                    &delays,
                    &le.dest,
                ),
                // Unreachable on the web: an export needs a destination path and there is none in
                // a browser, so `start_loop_export` returns before a capture ever begins.
                #[cfg(target_arch = "wasm32")]
                LoopFormat::Gif => Err(anyhow::anyhow!("GIF export needs a filesystem")),
                LoopFormat::Mp4 => {
                    crate::loopexport::encode_mp4_timed(&le.frames, &delays, &le.dest)
                }
            };
            // The sidecar: which scans the loop shows and how long each is held.
            if res.is_ok() {
                let v = &self.views[self.active];
                let (interval, fps) = crate::loopexport::timing_words(timing);
                let meta = serde_json::json!({
                    "site": v.site,
                    "product": v.moment.short_name(),
                    "tilt_index": v.tilt,
                    "format": match le.format { LoopFormat::Gif => "gif", LoopFormat::Mp4 => "mp4" },
                    "frame_px": le.frames.first().map(|f| [f.width(), f.height()]),
                    "interval": interval,
                    "fps": fps,
                    "duration_ms": delays.iter().map(|d| u64::from(*d)).sum::<u64>(),
                    "frames": frames,
                    "source": "NOAA NEXRAD Level II, rendered by HookEcho",
                });
                if let Ok(text) = serde_json::to_string_pretty(&meta) {
                    let _ = std::fs::write(le.dest.with_extension("json"), text);
                }
            }
            match res {
                Ok(()) => {
                    log::info!(
                        "loop saved: {} ({} frames)",
                        le.dest.display(),
                        le.frames.len()
                    );
                    let msg = format!("Loop saved ({} frames)", le.frames.len());
                    self.toast(ToastKind::Success, msg);
                }
                Err(e) => {
                    log::warn!("loop encode failed: {e}");
                    self.toast(ToastKind::Error, format!("Loop export failed: {e}"));
                }
            }
        }
    }
}

/// The longest a loop export waits for a stepped frame to be on screen before capturing what is.
const LOOP_FRAME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Whether the frame the timeline names is the one the pane shows, with nothing still loading.
fn frame_on_screen(shown: Option<&str>, wanted: Option<&str>, loading: bool) -> bool {
    !loading && wanted.is_some() && shown == wanted
}

#[cfg(test)]
mod tests {
    use super::frame_on_screen;

    #[test]
    fn a_frame_is_captured_only_once_it_is_the_one_shown() {
        assert!(frame_on_screen(Some("KTLX_2012"), Some("KTLX_2012"), false));
        // Still the previous scan: the duplicate a fixed wait used to capture.
        assert!(!frame_on_screen(
            Some("KTLX_2008"),
            Some("KTLX_2012"),
            false
        ));
        // The right scan, but a load still in flight may repaint it.
        assert!(!frame_on_screen(Some("KTLX_2012"), Some("KTLX_2012"), true));
        assert!(!frame_on_screen(None, Some("KTLX_2012"), false));
        assert!(!frame_on_screen(None, None, false));
    }
}
