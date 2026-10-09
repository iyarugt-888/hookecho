//! Loop export capture (GIF/MP4): step the timeline through every frame, wait for each to be
//! on screen, screenshot it, and encode. Moved out of `app.rs` (ROADMAP_2 §7).

use super::*;

/// In-progress loop export (GIF or MP4): steps the active timeline, grabbing one screenshot per
/// frame.
pub(crate) struct LoopExport {
    dest: std::path::PathBuf,
    format: crate::loopexport::LoopFormat,
    frames: Vec<image::RgbaImage>,
    /// Slots still to capture (counts down as frames are grabbed).
    remaining: usize,
    /// Frames to let the stepped radar paint once it is on screen, before grabbing.
    settle: u8,
    /// When the timeline was stepped to the frame being waited for (`loop_capture`).
    step_at: Instant,
    /// A screenshot has been requested; waiting for its event.
    capturing: bool,
    /// Playback speed the scrubber was set to when the export started — the exported clip plays
    /// at the speed the user was watching, instead of a hardcoded 5 fps.
    fps: f32,
    /// Each captured frame's volume and valid time, for real timing and the sidecar.
    volumes: Vec<Option<(String, DateTime<Utc>)>>,
    /// Hold each frame for its real scan gap (`Settings::loop_real_timing`) or all alike.
    real_timing: bool,
    /// What each captured frame asked for and found (ROADMAP_PARITY M6.3), for the manifest.
    records: Vec<crate::capture_manifest::FrameRecord>,
    /// The frame being captured: the scan asked for, and whether the wait for it ran out.
    asked: Option<String>,
    timed_out: bool,
    /// Logical frames in the loop, for "frame k of n".
    total: usize,
    /// The MP4's encoded frame rate (`Settings::loop_mp4_fps`).
    mp4_fps: u32,
    /// The scan the export is paused waiting for (1008.md F2): set when it did not appear within
    /// [`LOOP_FRAME_TIMEOUT`], until the person chooses what to do.
    paused: Option<String>,
    /// The person chose to capture what is shown for the frame waited on (the old behaviour,
    /// recorded as a substitution).
    take_shown: bool,
    /// Logical frames skipped rather than filled with another scan: (index, scan asked for).
    skipped: Vec<(usize, String)>,
}

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
            total: slots,
            mp4_fps: crate::loopexport::mp4_rate(self.settings.loop_mp4_fps),
            paused: None,
            take_shown: false,
            skipped: Vec::new(),
        });
    }

    /// Advance the loop export: wait for the stepped radar to settle, then request a screenshot.
    pub(crate) fn drive_loop_export(&mut self, ctx: &egui::Context) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        if le.capturing || le.paused.is_some() {
            return; // waiting for the screenshot event, or for the person
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
            if !le.take_shown {
                // Pause rather than record another scan under this frame's time (ROADMAP_PARITY
                // M6.3): the frames so far and a resumable manifest are kept, and the person
                // chooses (`loop_pause_window`).
                let waiting = wanted.clone().unwrap_or_default();
                log::warn!(
                    "loop export: {waiting} not on screen after {}s; paused",
                    LOOP_FRAME_TIMEOUT.as_secs()
                );
                le.paused = Some(waiting);
                let done = le.frames.len();
                self.write_partial_manifest();
                self.toast(
                    ToastKind::Info,
                    format!(
                        "Loop export paused at frame {} of {}: a scan did not load in {} s",
                        done + 1,
                        self.loop_export.as_ref().map_or(0, |l| l.total),
                        LOOP_FRAME_TIMEOUT.as_secs()
                    ),
                );
                return;
            }
            if le.settle == LOOP_SETTLE_FRAMES {
                le.timed_out = true;
                log::warn!(
                    "loop export: {} not on screen after {}s; capturing {} instead, as asked",
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
        le.take_shown = false;
        le.remaining -= 1;
        if le.remaining > 0 {
            self.views[self.active].timeline.step(1);
            if let Some(le) = &mut self.loop_export {
                le.settle = LOOP_SETTLE_FRAMES;
                le.step_at = Instant::now();
            }
        } else {
            let le = self.loop_export.take().unwrap();
            self.finish_loop_export(le, "complete");
        }
    }

    /// Encode the captured frames and write the sidecar manifest; `status` says whether every
    /// logical frame was reached ("complete") or the person finished early ("finished early").
    fn finish_loop_export(&mut self, le: LoopExport, status: &str) {
        {
            use crate::loopexport::{LoopFormat, Timing};
            if le.frames.is_empty() {
                self.toast(ToastKind::Info, "Loop export ended with no frames captured");
                return;
            }
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
                LoopFormat::Mp4 => crate::loopexport::encode_mp4_timed_at(
                    &le.frames, &delays, &le.dest, le.mp4_fps,
                ),
            };
            // The sidecar: which scans the loop shows and how long each is held, and the capture
            // manifest (ROADMAP_PARITY M6.3): each logical frame's encoded frames, what it asked
            // for and showed, its layers' times and a checksum, and the problems found.
            let encoded: Vec<u32> = match le.format {
                LoopFormat::Gif => vec![1; delays.len()],
                LoopFormat::Mp4 => crate::loopexport::cfr_counts(&delays, le.mp4_fps),
            };
            let problems = crate::capture_manifest::problems(&le.records);
            if res.is_ok() {
                let v = &self.views[self.active];
                let (interval, fps) = crate::loopexport::timing_words(timing);
                let meta = serde_json::json!({
                    "schema": crate::capture_manifest::MANIFEST_SCHEMA,
                    "encoded_fps": match le.format {
                        LoopFormat::Gif => None,
                        LoopFormat::Mp4 => Some(le.mp4_fps),
                    },
                    "logical_frames": crate::capture_manifest::frames(&le.records, &delays, &encoded),
                    "problems": problems,
                    "status": status,
                    "logical_frames_planned": le.total,
                    "skipped_frames": le.skipped.iter().map(|(i, s)| serde_json::json!({
                        "index": i, "asked": s,
                    })).collect::<Vec<_>>(),
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

impl LoopExport {
    /// Record the frame waited on as skipped and move past it; `true` while frames remain.
    fn skip_current(&mut self, waiting: String) -> bool {
        self.skipped.push((self.total - self.remaining, waiting));
        self.remaining -= 1;
        self.settle = LOOP_SETTLE_FRAMES;
        self.step_at = Instant::now();
        self.remaining > 0
    }

    /// The partial manifest while paused: the frames done, what each found, the scan waited for
    /// and the logical frame to resume at.
    fn partial_manifest(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": crate::capture_manifest::MANIFEST_SCHEMA,
            "status": "paused",
            "logical_frames_planned": self.total,
            "completed_frames": self.frames.len(),
            "resume_at_index": self.total - self.remaining,
            "waiting_for": self.paused,
            "timeout_s": LOOP_FRAME_TIMEOUT.as_secs(),
            "captured": self.records.iter().map(|r| serde_json::json!({
                "asked": r.asked,
                "shown": r.shown.as_ref().map(|(n, _)| n),
                "valid_time": r.shown.as_ref().map(|(_, t)| t.to_rfc3339()),
                "sha256": r.sha256,
            })).collect::<Vec<_>>(),
            "skipped_frames": self.skipped.iter().map(|(i, s)| serde_json::json!({
                "index": i, "asked": s,
            })).collect::<Vec<_>>(),
        })
    }
}

/// What the person chose for a paused frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PauseChoice {
    WaitAgain,
    Skip,
    CaptureShown,
    Finish,
}

impl HookEchoApp {
    /// The partial manifest of a paused export, beside its destination (`.partial.json`): the
    /// frames done, what each found, the scan waited for and where to resume, so a pause is never
    /// lost output.
    fn write_partial_manifest(&self) {
        let Some(le) = &self.loop_export else {
            return;
        };
        if let Ok(text) = serde_json::to_string_pretty(&le.partial_manifest()) {
            let _ = std::fs::write(le.dest.with_extension("partial.json"), text);
        }
    }

    /// The paused export's window: the scan waited for, and the four choices.
    pub(crate) fn loop_pause_window(&mut self, ctx: &egui::Context) {
        let Some(le) = &self.loop_export else {
            return;
        };
        let Some(waiting) = le.paused.clone() else {
            return;
        };
        let (done, total) = (le.frames.len(), le.total);
        let mut choice = None;
        egui::Window::new("Loop export paused")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Frame {} of {total} ({waiting}) did not load in {} s. {done} frame{} \
                     captured so far; a partial manifest is saved beside the file.",
                    done + 1,
                    LOOP_FRAME_TIMEOUT.as_secs(),
                    if done == 1 { "" } else { "s" }
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Wait again").clicked() {
                        choice = Some(PauseChoice::WaitAgain);
                    }
                    if ui
                        .button("Skip this frame")
                        .on_hover_text("Leave it out and record it as skipped")
                        .clicked()
                    {
                        choice = Some(PauseChoice::Skip);
                    }
                    if ui
                        .button("Capture what is shown")
                        .on_hover_text(
                            "Record the scan on screen in its place, listed as a problem",
                        )
                        .clicked()
                    {
                        choice = Some(PauseChoice::CaptureShown);
                    }
                    if ui
                        .add_enabled(done > 0, egui::Button::new("Finish with these"))
                        .clicked()
                    {
                        choice = Some(PauseChoice::Finish);
                    }
                });
            });
        if let Some(c) = choice {
            self.resolve_pause(c);
        }
    }

    pub(crate) fn resolve_pause(&mut self, choice: PauseChoice) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        let Some(waiting) = le.paused.take() else {
            return;
        };
        match choice {
            PauseChoice::WaitAgain => le.step_at = Instant::now(),
            PauseChoice::CaptureShown => le.take_shown = true,
            PauseChoice::Skip => {
                if le.skip_current(waiting) {
                    self.views[self.active].timeline.step(1);
                } else if let Some(le) = self.loop_export.take() {
                    self.finish_loop_export(le, "complete");
                }
            }
            PauseChoice::Finish => {
                if let Some(le) = self.loop_export.take() {
                    self.finish_loop_export(le, "finished early");
                }
            }
        }
    }

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

/// The longest a loop export waits for a stepped frame to be on screen before pausing
/// (ROADMAP_PARITY M6.3's default bound).
const LOOP_FRAME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether the frame the timeline names is the one the pane shows, with nothing still loading.
fn frame_on_screen(shown: Option<&str>, wanted: Option<&str>, loading: bool) -> bool {
    !loading && wanted.is_some() && shown == wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(total: usize) -> LoopExport {
        LoopExport {
            dest: std::path::PathBuf::from("loop.mp4"),
            format: crate::loopexport::LoopFormat::Mp4,
            frames: Vec::new(),
            remaining: total,
            settle: 0,
            step_at: Instant::now(),
            capturing: false,
            fps: 4.0,
            volumes: Vec::new(),
            real_timing: false,
            records: Vec::new(),
            asked: None,
            timed_out: false,
            total,
            mp4_fps: 30,
            paused: None,
            take_shown: false,
            skipped: Vec::new(),
        }
    }

    /// A paused export names where to resume and what it waits for; skipping records the frame
    /// by its index and moves on, and the last skip ends it (1008.md F2).
    #[test]
    fn a_pause_is_resumable_and_a_skip_is_recorded() {
        let mut le = export(3);
        le.remaining = 2; // one frame captured
        le.paused = Some("KTLX20240506_2312".into());
        let m = le.partial_manifest();
        assert_eq!(m["status"], "paused");
        assert_eq!(m["resume_at_index"], 1);
        assert_eq!(m["waiting_for"], "KTLX20240506_2312");
        assert_eq!(m["timeout_s"], 30);
        assert!(
            le.skip_current("KTLX20240506_2312".into()),
            "one frame left"
        );
        assert_eq!(le.skipped, vec![(1, "KTLX20240506_2312".to_string())]);
        assert_eq!(le.settle, LOOP_SETTLE_FRAMES);
        assert!(!le.skip_current("KTLX20240506_2316".into()), "nothing left");
        assert_eq!(le.partial_manifest()["skipped_frames"][1]["index"], 2);
    }

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
