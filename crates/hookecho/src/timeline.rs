//! Archive timeline / playback state for one map pane.
//!
//! Unifies live and archive under a single playhead: `following` pins the head to the newest
//! volume (live), and scrubbing/stepping un-pins it to browse a fixed list of volumes for the
//! selected UTC day. The app turns the current [`Timeline::current`] identifier into a decoded
//! volume (via an LRU cache + background download); this type is pure playback bookkeeping.

use chrono::{DateTime, NaiveDate, Utc};
use wxdata::clock::Instant;
use wxdata::level2::Identifier;

pub struct Timeline {
    /// Selected UTC archive day.
    pub date: NaiveDate,
    /// Volumes for the current site+date, oldest first.
    pub frames: Vec<Identifier>,
    /// Index into `frames` of the displayed volume.
    pub playhead: usize,
    /// Pinned to the newest volume (live). Cleared by scrubbing/stepping.
    pub following: bool,
    pub playing: bool,
    /// Playback rate in frames/second.
    pub speed: f32,
    pub loop_enabled: bool,
    /// How many newest frames the live loop cycles over (synced from settings each frame).
    pub live_window: usize,
    /// The (site, date) the current `frames` were listed for — detects a stale listing.
    pub frames_key: Option<(String, NaiveDate)>,
    /// A frame listing is in flight.
    pub listing: bool,
    /// After the next listing lands, snap the playhead to the frame nearest this time (event
    /// jump / archive deep-link). Cleared once applied.
    pub seek_target: Option<DateTime<Utc>>,
    /// How long a replay bundle's window is, in minutes. Set alongside `seek_target` when an
    /// event or a saved replay is opened; consumed when the listing lands, which is the first
    /// moment frame indices exist to bound.
    pub replay_span_min: u16,
    /// Playback bounds as frame indices, inclusive. `None` = play the whole listing. Any manual
    /// step or jump clears it: the user has taken the playhead back.
    pub replay: Option<(usize, usize)>,
    /// Forecast hours appended after the newest observed frame (HRRR "future radar" scrub tail).
    pub forecast_hours: u8,
    last_advance: Option<Instant>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            date: chrono::Utc::now().date_naive(),
            frames: Vec::new(),
            playhead: 0,
            following: true,
            playing: false,
            speed: 4.0,
            loop_enabled: true,
            live_window: 10,
            frames_key: None,
            listing: false,
            seek_target: None,
            replay_span_min: 0,
            replay: None,
            forecast_hours: 6,
            last_advance: None,
        }
    }
}

impl Timeline {
    /// The identifier of the volume at the playhead, if any (`None` in the forecast tail).
    pub fn current(&self) -> Option<&Identifier> {
        self.frames.get(self.playhead)
    }

    /// The newest known frame, regardless of where the playhead is — the site's own production
    /// cadence, not whatever a rolling live loop happens to be animating right now. [`current`]
    /// answers "what is on screen"; this answers "is the feed itself keeping up."
    ///
    /// [`current`]: Self::current
    pub fn newest(&self) -> Option<&Identifier> {
        self.frames.last()
    }

    /// Total scrub slots: observed frames plus the forecast tail (only when frames exist).
    pub fn slot_count(&self) -> usize {
        if self.frames.is_empty() {
            0
        } else {
            self.frames.len() + self.forecast_hours as usize
        }
    }

    /// If the playhead is in the forecast tail, the forecast hour (1..=forecast_hours), else None.
    pub fn forecast_hour(&self) -> Option<u8> {
        if !self.frames.is_empty() && self.playhead >= self.frames.len() {
            Some((self.playhead - self.frames.len() + 1) as u8)
        } else {
            None
        }
    }

    /// Whether the playhead is on (or past) the newest frame.
    pub fn at_head(&self) -> bool {
        self.frames.is_empty() || self.playhead + 1 >= self.frames.len()
    }

    /// Install a fresh frame listing; keeps the playhead at the head while following, else
    /// clamps it into range (so appended live frames don't move a scrubbed view). A listing for
    /// a different site never keeps the old index.
    pub fn set_frames(&mut self, frames: Vec<Identifier>, key: (String, NaiveDate)) {
        // A listing for a different site is a different axis: index N of one radar's day is not
        // the same moment as index N of another's. Carrying the playhead index across a site
        // switch is what drops a live loop hours into the archive — the loop window is a dozen
        // frames, a full day is hundreds, so index 8 lands just after 00Z instead of at the head.
        let switched_site = self.frames_key.as_ref().is_some_and(|(s, _)| *s != key.0);
        self.frames = frames;
        self.frames_key = Some(key);
        self.listing = false;
        // A pending event/deep-link seek wins: snap to the nearest frame by time. The target is
        // only consumed once a listing actually has a frame to land on — the first listing after
        // a site switch can arrive empty, and taking the seek there dropped it on the floor and
        // left the pane parked at the end of the day.
        if let Some(target) = self.seek_target {
            if let Some(i) = self.nearest_frame(target) {
                self.seek_target = None;
                self.playhead = i;
                self.following = false;
                // A replay bundle brackets the event rather than starting on it: the minutes
                // before are how you see the storm become the thing worth replaying.
                let span = std::mem::take(&mut self.replay_span_min);
                if span > 0 {
                    let half = chrono::Duration::minutes(span as i64 / 2);
                    let from = self.nearest_frame(target - half).unwrap_or(0);
                    let to = self
                        .nearest_frame(target + half)
                        .unwrap_or(self.frames.len().saturating_sub(1));
                    let (from, to) = (from.min(to), to.max(from));
                    // A window that collapsed onto one frame is not a replay: a listing that
                    // thin would loop the same volume forever, which reads as a frozen app.
                    if to > from {
                        self.replay = Some((from, to));
                        self.playhead = from;
                        self.playing = true;
                        self.loop_enabled = true;
                    }
                }
                return;
            }
        }
        // A listing for another site or day is a different axis; the old bounds mean nothing on it.
        if switched_site {
            self.replay = None;
        }
        // While live-looping, a fresh listing must not yank the playhead to the head — the loop
        // owns the playhead. Only clamp it back into range if it now points past the end.
        if self.following && self.playing && !switched_site && !self.frames.is_empty() {
            self.playhead = self.playhead.min(self.frames.len() - 1);
        } else if self.following || self.playhead >= self.frames.len() {
            self.playhead = self.frames.len().saturating_sub(1);
        }
        self.pin_to_live_window();
    }

    /// Pull the playhead back inside the live window when a fresh listing is installed.
    ///
    /// `following` means "showing live": the newest volume, or — while looping — one of the
    /// newest `live_window`. A listing replaces the whole axis under the playhead, and nothing
    /// used to check the index still meant anything afterwards: a day listing that arrives
    /// hundreds of frames long, or a window shrunk in settings, left the LIVE badge lit over a
    /// volume hours old. Deliberately not applied to `append_head`/`tick` — a head arriving
    /// mid-loop slides the window on the next wrap, which is what
    /// `appended_head_slides_the_window` pins down.
    fn pin_to_live_window(&mut self) {
        if !self.following || self.frames.is_empty() {
            return;
        }
        let oldest_live = self.frames.len().saturating_sub(self.live_window.max(1));
        self.playhead = self.playhead.clamp(oldest_live, self.frames.len() - 1);
    }

    /// Append a newly-arrived live head frame (keeps the loop window sliding forward). The
    /// playhead is left where it is; the window is relative to `frames.len()`.
    pub fn append_head(&mut self, id: Identifier) {
        self.frames.push(id);
    }

    /// True when the play button is running a rolling live loop (pinned to head, playing, and the
    /// playhead is somewhere behind the newest frame).
    pub fn live_looping(&self) -> bool {
        self.following && self.playing && !self.at_head()
    }

    /// Play/pause toggle. Starting at the live head begins a rolling loop over the newest
    /// `live_window` frames while staying pinned to live; starting on an archive day replays from
    /// the first frame.
    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if self.playing && self.at_head() && !self.frames.is_empty() {
            if self.following {
                // Live: loop the tail window, stay pinned so new volumes keep arriving.
                self.playhead = self.frames.len().saturating_sub(self.live_window.max(1));
            } else {
                self.playhead = 0; // archive: replay from the start
            }
        }
    }

    /// Index of the frame whose time is closest to `target`.
    fn nearest_frame(&self, target: DateTime<Utc>) -> Option<usize> {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                id.date_time()
                    .map(|t| (i, (t - target).num_seconds().abs()))
            })
            .min_by_key(|(_, d)| *d)
            .map(|(i, _)| i)
    }

    /// Jump straight to the volume nearest `hour:minute` UTC on the currently-selected day.
    ///
    /// Unlike `seek_target` (which waits for the *next* listing to land, for a day that has not
    /// been fetched yet), this acts on `frames` immediately — the picker that calls it only shows
    /// once a day is already listed, so there is nothing to wait for. Silently does nothing for
    /// an out-of-range `hour`/`minute` or an empty listing, rather than fail a mistyped value.
    pub fn seek_to_time_of_day(&mut self, hour: u32, minute: u32) {
        let Some(target) = self
            .date
            .and_hms_opt(hour, minute, 0)
            .map(|t| t.and_utc())
        else {
            return;
        };
        let Some(i) = self.nearest_frame(target) else {
            return;
        };
        self.playing = false;
        self.replay = None;
        self.playhead = i;
        self.following = i + 1 == self.frames.len();
    }

    /// Jump forward/backward by about `minutes` of archive time from the playhead's own frame —
    /// the coarse counterpart to `step`'s one-frame-at-a-time, for closing distance across a whole
    /// day quickly. Landing is by nearest frame, same as `seek_to_time_of_day`, so it lands close
    /// to a clean value regardless of the site's actual volume cadence.
    ///
    /// Confined to the day already listed, same as `step`: a request that would cross into the
    /// day before or after just clamps to that end of `frames` rather than reaching for a listing
    /// this call has no way to wait for.
    pub fn step_time(&mut self, minutes: i64) {
        let Some(cur) = self.current().and_then(|id| id.date_time()) else {
            return;
        };
        let target = cur + chrono::Duration::minutes(minutes);
        let Some(i) = self.nearest_frame(target) else {
            return;
        };
        self.playing = false;
        self.replay = None;
        self.playhead = i;
        self.following = i + 1 == self.frames.len();
    }

    /// Step `delta` slots (observed frames + forecast tail), un-pinning and pausing playback.
    pub fn step(&mut self, delta: i32) {
        self.playing = false;
        self.replay = None;
        let n = self.slot_count() as i32;
        if n == 0 {
            return;
        }
        self.playhead = (self.playhead as i32 + delta).clamp(0, n - 1) as usize;
        // Re-pin to live only when back on the last observed frame (not in the forecast tail).
        self.following = self.playhead + 1 == self.frames.len();
    }

    /// Jump to the newest frame and re-pin to live.
    pub fn go_head(&mut self) {
        self.replay = None;
        self.following = true;
        self.playing = false;
        self.playhead = self.frames.len().saturating_sub(1);
    }

    /// Jump to the oldest frame.
    pub fn go_begin(&mut self) {
        self.following = false;
        self.playing = false;
        self.playhead = 0;
    }

    /// How long until the next playback frame is due, or `None` when not playing. The UI turns
    /// this into a repaint deadline — without it, playback advances only as fast as frames
    /// happen to be drawn, which the idle heartbeat caps at 4/s on Android and 10/s on desktop
    /// no matter what speed the user picked.
    pub fn time_to_next_frame(&self) -> Option<std::time::Duration> {
        if !self.playing || self.frames.is_empty() {
            return None;
        }
        let interval = self.frame_interval();
        Some(match self.last_advance {
            Some(t) => interval.saturating_sub(t.elapsed()),
            None => std::time::Duration::ZERO,
        })
    }

    fn frame_interval(&self) -> std::time::Duration {
        frame_interval(self.speed, crate::ui::motion::degraded())
    }

    /// Advance playback if a frame interval has elapsed. Returns true if the playhead moved.
    pub fn tick(&mut self) -> bool {
        if !self.playing || self.frames.is_empty() {
            return false;
        }
        let interval = self.frame_interval();
        if self.last_advance.is_some_and(|t| t.elapsed() < interval) {
            return false;
        }
        self.last_advance = Some(Instant::now());
        // A replay bundle owns the playhead: it wraps inside its own window, live or not.
        if let Some((from, to)) = self.replay {
            let to = to.min(self.frames.len().saturating_sub(1));
            self.playhead = if self.playhead >= to {
                from.min(to)
            } else {
                self.playhead + 1
            };
            return true;
        }
        if self.playhead + 1 < self.frames.len() {
            self.playhead += 1;
            // Archive play re-pins to live at the head; a live loop stays pinned throughout.
            if !self.following {
                self.following = self.playhead + 1 >= self.frames.len();
            }
        } else if self.loop_enabled {
            // Wrap: a live loop jumps back to the window start and keeps following; an archive
            // loop restarts from the beginning, un-pinned.
            if self.following {
                self.playhead = self.frames.len().saturating_sub(self.live_window.max(1));
            } else {
                self.playhead = 0;
            }
        } else {
            self.playing = false;
        }
        true
    }
}

fn frame_interval(speed: f32, degraded: bool) -> std::time::Duration {
    let speed = if degraded { speed.min(8.0) } else { speed };
    std::time::Duration::from_secs_f32((1.0 / speed).clamp(0.05, 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // `Identifier` has no cheap public constructor, so the populated-frame paths are exercised
    // by the app integration; here we lock the empty-list safety and the index arithmetic that
    // `step`/`tick` rely on.
    #[test]
    fn no_repaint_deadline_when_not_playing() {
        let mut t = Timeline::default();
        assert!(t.time_to_next_frame().is_none(), "idle asks for nothing");
        // Still nothing to pace with an empty frame list, even once "playing".
        t.playing = true;
        assert!(t.time_to_next_frame().is_none());
    }

    #[test]
    fn visual_guard_caps_playback_at_eight_fps() {
        assert_eq!(super::frame_interval(16.0, true).as_millis(), 125);
        assert!(super::frame_interval(16.0, false).as_millis() < 125);
        assert_eq!(super::frame_interval(4.0, true).as_millis(), 250);
    }

    /// A day of volumes for `site`, one every five minutes from 00Z.
    fn day(site: &str, n: usize) -> Vec<Identifier> {
        (0..n)
            .map(|i| {
                let (h, m) = (i * 5 / 60, i * 5 % 60);
                Identifier::new(format!("{site}20260819_{h:02}{m:02}00_V06"))
            })
            .collect()
    }

    #[test]
    fn switching_sites_mid_loop_does_not_land_in_the_small_hours() {
        let mut t = Timeline::default();
        let today = t.date;
        // A live loop over the newest dozen frames of one radar's day.
        t.set_frames(day("KTLX", 280), ("KTLX".into(), today));
        t.toggle_play();
        assert!(t.live_looping(), "looping the tail of KTLX");
        let looped_at = t.playhead;
        assert!(looped_at > 12, "loop starts near the head, not the start");

        // The same index in another radar's listing is a different moment entirely.
        t.set_frames(day("KFWS", 24), ("KFWS".into(), today));
        assert_eq!(
            t.playhead, 23,
            "new site starts at its head, not index {looped_at}"
        );
        assert!(t.following);
    }

    #[test]
    fn a_scrubbed_pane_keeps_its_time_across_a_site_switch() {
        let mut t = Timeline::default();
        let today = t.date;
        t.set_frames(day("KTLX", 280), ("KTLX".into(), today));
        t.step(-100); // scrub back; un-pins from live
        assert!(!t.following);
        let want = t.current().unwrap().date_time().unwrap();

        // The app clears the old site's frames and hands the time over as a seek target.
        t.seek_target = Some(want);
        t.frames.clear();
        t.playhead = 0;
        t.set_frames(day("KFWS", 280), ("KFWS".into(), today));
        assert_eq!(t.current().unwrap().date_time(), Some(want));
    }

    #[test]
    fn live_never_installs_a_listing_older_than_the_loop_window() {
        let mut t = Timeline::default();
        let today = t.date;
        t.set_frames(day("KFTG", 24), ("KFTG".into(), today));
        t.live_window = 10;
        t.playing = true;
        t.playhead = 2; // looping near the start of a short listing

        // The day's full listing lands: the same index is now the small hours of the morning.
        t.set_frames(day("KFTG", 280), ("KFTG".into(), today));
        assert!(
            t.playhead >= 270,
            "LIVE must land inside the live window, got {}",
            t.playhead
        );

        // A scrubbed timeline is not pinned and keeps the frame the user chose.
        t.following = false;
        t.playhead = 2;
        t.set_frames(day("KFTG", 280), ("KFTG".into(), today));
        assert_eq!(t.playhead, 2, "archive scrubbing is untouched");
    }

    #[test]
    fn empty_timeline_is_safe() {
        let mut t = Timeline::default();
        assert!(t.at_head());
        t.step(3);
        assert_eq!(t.playhead, 0);
        t.go_head();
        assert_eq!(t.playhead, 0);
        assert!(!t.tick(), "no playback without frames");
    }

    #[test]
    fn playhead_math_clamps_and_loops() {
        let n = 5usize;
        let clamp = |p: i32| p.clamp(0, n as i32 - 1) as usize;
        assert_eq!(clamp(-2), 0);
        assert_eq!(clamp(9), 4);
        let next = |p: usize| if p + 1 < n { p + 1 } else { 0 };
        assert_eq!(next(4), 0);
        assert_eq!(next(2), 3);
    }

    /// Build a timeline with `n` valid archive frames (names parse to real times).
    fn with_frames(n: usize) -> Timeline {
        Timeline {
            frames: (0..n)
                .map(|i| Identifier::new(format!("KTLX20130520_{:02}{:02}00_V06", i / 60, i % 60)))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn toggle_play_at_live_head_starts_rolling_loop() {
        let mut t = with_frames(15);
        t.live_window = 10;
        t.following = true;
        t.playhead = 14; // at head
        t.toggle_play();
        assert!(t.playing);
        assert!(t.following, "live loop stays pinned to the head");
        assert_eq!(t.playhead, 5, "loop starts at len - window");
    }

    #[test]
    fn live_loop_wraps_to_window_start_and_keeps_following() {
        let mut t = with_frames(15);
        t.live_window = 10;
        t.following = true;
        t.playing = true;
        t.playhead = 14; // last frame → tick wraps
        assert!(t.tick(), "first tick fires immediately");
        assert_eq!(t.playhead, 5, "wrap to len - window");
        assert!(t.following, "still live");
    }

    #[test]
    fn archive_loop_wraps_to_zero_unpinned() {
        let mut t = with_frames(5);
        t.following = false;
        t.playing = true;
        t.playhead = 4;
        assert!(t.tick());
        assert_eq!(t.playhead, 0, "archive loop restarts from the beginning");
        assert!(!t.following);
    }

    #[test]
    fn appended_head_slides_the_window() {
        let mut t = with_frames(12);
        t.live_window = 10;
        t.following = true;
        t.playing = true;
        t.playhead = 2; // mid-window
        let before = t.frames.len();
        t.append_head(Identifier::new("KTLX20130520_0012_00_V06".to_string()));
        assert_eq!(t.frames.len(), before + 1, "head appended");
        assert_eq!(
            t.playhead, 2,
            "playhead unmoved; window slides on next wrap"
        );
        assert!(t.live_looping());
    }

    #[test]
    fn a_seek_survives_a_listing_it_cannot_land_on() {
        let mut t = Timeline::default();
        let day_frames = day("KTLX", 288);
        let target = day_frames[144].date_time().expect("frame time parses");
        t.seek_target = Some(target);
        t.replay_span_min = 60;
        // The first listing after a site switch can arrive empty; the seek must outlive it.
        t.set_frames(Vec::new(), ("KTLX".into(), t.date));
        assert_eq!(t.seek_target, Some(target));
        assert_eq!(t.replay_span_min, 60);

        t.set_frames(day_frames, ("KTLX".into(), t.date));
        assert_eq!(t.seek_target, None);
        assert_eq!(t.replay, Some((138, 150)));
    }

    #[test]
    fn a_listing_too_thin_to_bracket_does_not_loop_one_frame_forever() {
        let mut t = Timeline::default();
        let frames = day("KTLX", 1);
        let target = frames[0].date_time().expect("frame time parses");
        t.seek_target = Some(target);
        t.replay_span_min = 60;
        t.set_frames(frames, ("KTLX".into(), t.date));
        // A single-volume listing: both ends of the window land on it. Seek there, but do not
        // call it a replay.
        assert_eq!(t.replay, None);
        assert!(!t.playing);
    }

    #[test]
    fn a_replay_bundle_brackets_its_event_and_loops_inside_the_window() {
        let mut t = Timeline::default();
        let day_frames = day("KTLX", 288); // 24 h at 5-minute volumes
        let target = day_frames[144].date_time().expect("frame time parses"); // 12:00Z
        t.seek_target = Some(target);
        t.replay_span_min = 60;
        t.set_frames(day_frames, ("KTLX".into(), t.date));

        // Half an hour either side of noon, at 5-minute frames: 6 frames back, 6 forward.
        assert_eq!(t.replay, Some((138, 150)));
        assert_eq!(t.playhead, 138, "starts before the event, not on it");
        assert!(t.playing && t.loop_enabled);

        // Playback wraps at the end of the window instead of running on into the afternoon.
        t.playhead = 150;
        t.last_advance = None;
        assert!(t.tick());
        assert_eq!(t.playhead, 138);

        // Taking the playhead back by hand ends the bundle.
        t.step(1);
        assert_eq!(t.replay, None);
    }

    #[test]
    fn seek_to_time_of_day_lands_on_the_nearest_frame_and_unpins_from_live() {
        let mut t = Timeline::default();
        // `day()` always stamps 2026-08-19 into its identifiers regardless of `t.date` (its
        // callers only ever check playhead/index arithmetic, not the wall-clock date) — this
        // test is the first to seek by time of day, which needs `t.date` to actually agree with
        // the frames' own embedded dates, or every frame looks equally far from the target.
        let day_of = chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap();
        t.date = day_of;
        // Five-minute volumes all day: 12:32Z is 2 minutes past the 12:30 frame and 3 short of
        // 12:35, so the nearest one is 12:30.
        t.set_frames(day("KTLX", 288), ("KTLX".into(), day_of));
        t.following = true;
        t.playing = true;
        t.seek_to_time_of_day(12, 32);
        assert!(!t.playing, "typing a time takes manual control, like step()");
        assert!(!t.following, "no longer pinned to the live head");
        assert_eq!(
            t.current().unwrap().date_time().unwrap().format("%H:%M").to_string(),
            "12:30"
        );
    }

    #[test]
    fn seek_to_time_of_day_is_a_no_op_on_an_empty_listing() {
        let mut t = Timeline::default();
        t.seek_to_time_of_day(12, 0); // nothing to land on; must not panic
        assert_eq!(t.playhead, 0);
    }

    #[test]
    fn step_time_jumps_about_an_hour_and_clamps_at_the_day() {
        let mut t = Timeline::default();
        let today = t.date;
        t.set_frames(day("KTLX", 288), ("KTLX".into(), today)); // 5-minute volumes, 24 h
        t.playhead = 150; // 12:30Z
        t.playing = true;

        t.step_time(60);
        assert!(!t.playing, "a jump takes manual control, like step()");
        assert_eq!(t.playhead, 162, "12:30 + 1h = 13:30, 12 frames on at 5 min each");

        t.step_time(-120);
        assert_eq!(t.playhead, 138, "13:30 - 2h = 11:30");

        // Requesting past either end of the day lands on that end rather than reaching for a
        // listing this call has no way to wait for.
        t.playhead = 0; // 00:00Z
        t.step_time(-60);
        assert_eq!(t.playhead, 0, "nothing earlier in today's listing");
        t.playhead = 287; // 23:55Z
        t.step_time(60);
        assert_eq!(t.playhead, 287, "nothing later in today's listing");
    }

    #[test]
    fn step_time_is_a_no_op_on_an_empty_listing() {
        let mut t = Timeline::default();
        t.step_time(60); // nothing to land on; must not panic
        assert_eq!(t.playhead, 0);
    }

    /// `newest` must answer "what has the feed produced" and not "what is on screen": a rolling
    /// live loop leaves the playhead well behind the newest frame on purpose (the whole point of
    /// looping the tail window), and the staleness badge/age readout that reads this needs the
    /// feed's own cadence, not the loop's current animation position.
    #[test]
    fn newest_is_the_last_frame_regardless_of_the_playhead() {
        let mut t = Timeline::default();
        let today = t.date;
        t.set_frames(day("KTLX", 12), ("KTLX".into(), today));
        t.toggle_play();
        assert!(t.live_looping(), "looping the tail, playhead behind the head");
        assert_ne!(
            t.current().map(Identifier::name),
            t.newest().map(Identifier::name),
            "the loop is not currently showing the newest frame"
        );
        assert_eq!(
            t.newest().map(Identifier::name),
            t.frames.last().map(Identifier::name),
            "newest is always the last listed frame, wherever the playhead sits"
        );
    }

    #[test]
    fn newest_is_none_on_an_empty_listing() {
        assert!(Timeline::default().newest().is_none());
    }
}
