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
            records: Vec::with_capacity(slots),
            asked: None,
            timed_out: false,
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
                le.timed_out = true;
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
        le.asked = wanted;
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
        let sha256 = crate::capture_manifest::checksum(&buf);
        if let Some(img) = image::RgbaImage::from_raw(w, h, buf) {
            le.frames.push(img);
            // What is on screen, not what the cursor names: the label must match the picture.
            let shown = self.views[self.active]
                .volume
                .as_ref()
                .map(|v| (v.name.clone(), v.time));
            le.volumes.push(shown.clone());
            let record = crate::capture_manifest::FrameRecord {
                asked: le.asked.take(),
                shown,
                waited_ms: le.step_at.elapsed().as_millis() as u64,
                timed_out: std::mem::take(&mut le.timed_out),
                sources: Vec::new(),
                sha256,
            };
            let sources = self.capture_sources(record.shown.as_ref().map(|(_, t)| *t));
            if let Some(le) = &mut self.loop_export {
                le.records
                    .push(crate::capture_manifest::FrameRecord { sources, ..record });
            }
        }
        let Some(le) = &mut self.loop_export else {
            return;
        };
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
            // The sidecar: which scans the loop shows and how long each is held, and the capture
            // manifest (ROADMAP_PARITY M6.3): each logical frame's encoded frames, what it asked
            // for and showed, its layers' times and a checksum, and the problems found.
            let encoded: Vec<u32> = match le.format {
                LoopFormat::Gif => vec![1; delays.len()],
                LoopFormat::Mp4 => {
                    crate::loopexport::cfr_counts(&delays, crate::loopexport::MP4_FPS)
                }
            };
            let problems = crate::capture_manifest::problems(&le.records);
            if res.is_ok() {
                let v = &self.views[self.active];
                let (interval, fps) = crate::loopexport::timing_words(timing);
                let meta = serde_json::json!({
                    "schema": crate::capture_manifest::MANIFEST_SCHEMA,
                    "encoded_fps": match le.format {
                        LoopFormat::Gif => None,
                        LoopFormat::Mp4 => Some(crate::loopexport::MP4_FPS),
                    },
                    "logical_frames": crate::capture_manifest::frames(&le.records, &delays, &encoded),
                    "problems": problems,
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
                    if problems.is_empty() {
                        let msg = format!("Loop saved ({} frames)", le.frames.len());
                        self.toast(ToastKind::Success, msg);
                    } else {
                        // Saved, but not as asked: say so, and where the details are.
                        let msg = format!(
                            "Loop saved ({} frames) with {} problem{}: {}. Details in the .json",
                            le.frames.len(),
                            problems.len(),
                            if problems.len() == 1 { "" } else { "s" },
                            problems[0]
                        );
                        self.toast(ToastKind::Info, msg);
                    }
                }
                Err(e) => {
                    log::warn!("loop encode failed: {e}");
                    self.toast(ToastKind::Error, format!("Loop export failed: {e}"));
                }
            }
        }
    }
}

impl HookEchoApp {
    /// Every field layer on the active pane as a capture sees it: ready for the pane's selected
    /// time or not, and its data's valid time.
    fn capture_sources(
        &self,
        radar_time: Option<DateTime<Utc>>,
    ) -> Vec<crate::capture_manifest::SourceStamp> {
        let v = &self.views[self.active];
        crate::capture_manifest::stamps(
            radar_time,
            crate::render::FieldLayer::DRAW_ORDER
                .iter()
                .filter(|l| v.fields_on.contains(l))
                .map(|l| {
                    let valid = self
                        .field_state_for(self.active, *l)
                        .and_then(|s| s.stamp.as_ref())
                        .map(|s| s.valid_time);
                    (
                        l.slug().to_string(),
                        self.mrms_ready_for(self.active, *l),
                        valid,
                    )
                }),
        )
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
