//! The max/min trail layer (ROADMAP_PARITY M3.4): the exact extremum each gate held over the
//! window ending at the pane's playhead, from the loop's cached volumes.
//!
//! The trail is [`wxdata::extrema::SlidingTrail`]: frames are kept by their scan time and the
//! trail is recomputed from those inside the window, so stepping the playhead forward past the
//! strongest old frame removes it, and stepping back gives exactly what a trail built fresh at
//! that time would. Values are never rewritten by age. "Fade with age" is display only: a per-gate
//! opacity from the time of the frame that supplied each gate (`RadarUpload::gate_alpha`), so the
//! probe, threshold, outline and GeoTIFF export read the same physical extrema with it on or off.
use super::*;
use wxdata::extrema::{Coverage, Extremum, Merge, Mismatch, SlidingTrail, WindowTrail};

/// Frames one trail holds at most; the window bounds it first. A 120-minute window at a
/// four-minute cadence is 30 frames, about 1.3 MB each at super-resolution.
const MAX_FRAMES: usize = if cfg!(target_os = "android") { 32 } else { 64 };

/// Display opacity at the old end of the window when fading: still readable, clearly older.
const FADE_FLOOR: u8 = 64;

/// What a trail is built for; any change starts it over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TrailKey {
    data: usize,
    site: Option<String>,
    moment: Moment,
    tilt: usize,
    pub(super) keep: Extremum,
    pub(super) window_min: u16,
}

impl TrailKey {
    pub(super) fn moment(&self) -> Moment {
        self.moment
    }
}

pub(crate) struct TrailState {
    pub(super) key: TrailKey,
    trail: SlidingTrail,
    /// Frames offered and left out (could not be binned, or another beam than the newest), by
    /// name, so each is tried once; cleared when the trail resets.
    skipped: std::collections::HashSet<String>,
    /// The playhead time (seconds) the shown trail is as of.
    pub(super) anchor: Option<i64>,
    pub(super) shown: Option<WindowTrail>,
    /// Per-gate display opacity for `shown` while fading.
    alpha: Option<Vec<u8>>,
    fade: bool,
    /// Bumped whenever `shown` or its fade changes, so the shown-image key moves with it.
    generation: u32,
}

/// Display opacity per gate from its contributor's age: opaque for the frame at `now`, falling
/// linearly to [`FADE_FLOOR`] at the old end of the window. A gate with no contributor has no
/// value to draw, and is left opaque (its code is transparent already).
pub(crate) fn age_alpha(contributor: &[Option<i64>], now: i64, window_s: i64) -> Vec<u8> {
    let span = window_s.max(1) as f32;
    contributor
        .iter()
        .map(|who| match who {
            Some(t) => {
                let age = ((now - t).max(0) as f32 / span).min(1.0);
                (255.0 - age * (255.0 - f32::from(FADE_FLOOR))).round() as u8
            }
            None => 255,
        })
        .collect()
}

/// The coverage a status line owes the reader: a history shorter than the window, and volumes
/// missing at the trail's own cadence.
fn coverage_note(c: &Coverage) -> String {
    let mut out = String::new();
    if c.short_s > 0 {
        out.push_str(&format!(
            ", history covers {} of {} min",
            ((c.requested_s - c.short_s) / 60).max(0),
            c.requested_s / 60
        ));
    }
    if c.missing > 0 {
        out.push_str(&format!(
            ", {} volume{} missing",
            c.missing,
            if c.missing == 1 { "" } else { "s" }
        ));
    }
    out
}

pub(super) fn trail_status_line(
    folded: usize,
    wanted: usize,
    window_min: u16,
    keep: Extremum,
    restarted: Option<Mismatch>,
) -> String {
    let what = match keep {
        Extremum::Max => "maximum",
        Extremum::Min => "minimum",
    };
    let mut line = if folded < wanted {
        format!("Building {what} trail: {folded} of {wanted} cached volumes")
    } else {
        format!("{what} of {wanted} cached volumes over {window_min} min")
    };
    if let Some(why) = restarted {
        let why = match why {
            Mismatch::Moment => "the product changed",
            Mismatch::ValueRange => "raw and dealiased velocity differ",
            Mismatch::Geometry => "the scan geometry changed",
            Mismatch::Elevation => "the tilt changed",
            Mismatch::Site => "the site changed",
        };
        line.push_str(&format!(" — restarted: {why}"));
    }
    line
}

impl TrailState {
    /// The display opacity to upload with the trail, when fading.
    pub(super) fn gate_alpha(&self) -> Option<&Vec<u8>> {
        self.alpha.as_ref()
    }
}

impl HookEchoApp {
    /// Bring the active pane's trail up to date and return the tag that makes the shown-image key
    /// change with it, or `None` when there is nothing to draw.
    ///
    /// At most [`Self::TRAIL_FOLDS_PER_FRAME`] volumes are binned per UI frame: binning is real
    /// work, and a two-hour window is two dozen of them, so the image grows over a few frames
    /// instead of stalling one.
    pub(crate) fn advance_trail(
        &mut self,
        data: usize,
        moment: Moment,
        tilt: usize,
    ) -> Option<String> {
        let window = self.filters.trail_window_min;
        let window_s = i64::from(window) * 60;
        let keep = if self.filters.trail_keep_min {
            Extremum::Min
        } else {
            Extremum::Max
        };
        // Runs every UI frame while the layer is on, so names are cloned only for the frames
        // inside the window rather than for the whole day's timeline.
        let (anchor, in_window): (Option<i64>, Vec<(String, i64)>) = {
            let tl = &self.views[data].timeline;
            let upto = &tl.frames[..(tl.playhead + 1).min(tl.frames.len())];
            let anchor = upto
                .iter()
                .rev()
                .find_map(|id| id.date_time())
                .map(|t| t.timestamp());
            let names = anchor.map_or_else(Vec::new, |a| {
                upto.iter()
                    .filter_map(|id| Some((id.name().to_string(), id.date_time()?.timestamp())))
                    .filter(|(_, t)| *t <= a && *t >= a - window_s)
                    .collect()
            });
            (anchor, names)
        };
        let wanted: Vec<(String, i64)> = in_window
            .into_iter()
            .filter(|(name, _)| self.scan_cache.contains(name))
            .collect();
        let Some(anchor) = anchor.filter(|_| !wanted.is_empty()) else {
            self.trail = None;
            self.trail_more = false;
            return None;
        };
        let key = TrailKey {
            data,
            site: self.views[data].site.clone(),
            moment,
            tilt,
            keep,
            window_min: window,
        };
        if self.trail.as_ref().is_none_or(|t| t.key != key) {
            let generation = self
                .trail
                .as_ref()
                .map_or(0, |t| t.generation.wrapping_add(1));
            self.trail = Some(TrailState {
                key,
                trail: SlidingTrail::new(keep, window_s, MAX_FRAMES),
                skipped: Default::default(),
                anchor: None,
                shown: None,
                alpha: None,
                fade: false,
                generation,
            });
        }
        let fade = self.filters.trail_decay;
        let state = self.trail.as_mut()?;
        let mut changed = state.anchor != Some(anchor);
        if changed {
            state.trail.retain_window(anchor);
            state.anchor = Some(anchor);
        }
        let mut binned = 0;
        for (name, t) in &wanted {
            if state.trail.times().any(|h| h == *t) || state.skipped.contains(name) {
                continue;
            }
            if binned == Self::TRAIL_FOLDS_PER_FRAME {
                break;
            }
            binned += 1;
            changed = true;
            let Some(scan) = self.scan_cache.peek(name).map(Arc::clone) else {
                state.skipped.insert(name.clone());
                continue;
            };
            match level2::bin_scan(&scan, moment, tilt) {
                Ok(sweep) => match state.trail.push(*t, &sweep) {
                    Merge::Merged => {}
                    Merge::Skipped(_) => {
                        state.skipped.insert(name.clone());
                    }
                    Merge::Reset(_) => state.skipped.clear(),
                },
                Err(e) => {
                    log::debug!("trail: skipping {name}: {e}");
                    state.skipped.insert(name.clone());
                }
            }
        }
        let pending = wanted
            .iter()
            .filter(|(name, t)| {
                !state.trail.times().any(|h| h == *t) && !state.skipped.contains(name)
            })
            .count();
        self.trail_more = pending > 0;
        if changed || state.fade != fade {
            state.shown = state.trail.at(anchor);
            state.alpha = state
                .shown
                .as_ref()
                .filter(|_| fade)
                .map(|w| age_alpha(&w.contributor, anchor, window_s));
            state.fade = fade;
            state.generation = state.generation.wrapping_add(1);
        }
        let mut status = trail_status_line(
            wanted.len() - pending,
            wanted.len(),
            window,
            keep,
            state.trail.last_reset,
        );
        if let Some(w) = &state.shown {
            status.push_str(&coverage_note(&w.coverage));
        }
        if fade {
            status.push_str(", older gates drawn fainter (values unchanged)");
        }
        self.filters.trail_status = status;
        state
            .shown
            .is_some()
            .then(|| format!("trail{}", state.generation))
    }

    /// The trail's reading at `(lon, lat)` on pane `idx`, when that pane is drawing it: the
    /// physical extremum, when the frame that supplied it was scanned, and the window behind it.
    pub(super) fn trail_probe_line(
        &self,
        idx: usize,
        lon: f64,
        lat: f64,
    ) -> Option<super::layer_probe::ProbeLine> {
        if !self.filters.show_trail || idx != self.active {
            return None;
        }
        let state = self.trail.as_ref()?;
        let shown = state.shown.as_ref()?;
        let anchor = state.anchor?;
        let (sample, who) = shown.at_point(lon, lat)?;
        let moment = state.key.moment;
        let what = match state.key.keep {
            Extremum::Max => "Max",
            Extremum::Min => "Min",
        };
        let value =
            super::radar_probe::format_value(moment, sample.value, self.settings.velocity_unit)
                .unwrap_or_else(|| "\u{2014}".into());
        let tz = self.active_tz();
        let mut detail = match who.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
            Some(t) => format!(
                "from the {} scan, {} min before the playhead",
                crate::timefmt::fmt_clock(t, tz, true),
                (anchor - t.timestamp()) / 60
            ),
            None => "no frame in the window measured this gate".into(),
        };
        let c = &shown.coverage;
        detail.push_str(&format!(
            "; {} frame{} over {} min{}",
            c.frames,
            if c.frames == 1 { "" } else { "s" },
            (c.to - c.from) / 60,
            coverage_note(c)
        ));
        Some(super::layer_probe::ProbeLine::new(
            format!(
                "{what} {} trail {} min",
                moment.short_name(),
                state.key.window_min
            ),
            value,
            Some(detail),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_is_opacity_from_opaque_now_to_the_floor_at_the_window_edge() {
        let a = age_alpha(&[Some(1_000), Some(700), Some(400), None], 1_000, 600);
        assert_eq!(a[0], 255, "the newest frame's gates are opaque");
        assert_eq!(a[2], FADE_FLOOR, "the window's oldest are at the floor");
        assert!(a[1] < 255 && a[1] > FADE_FLOOR, "{}", a[1]);
        assert_eq!(a[3], 255, "no contributor: nothing to fade");
        // A frame scanned after `now` (never shown, but never a panic) reads as new.
        assert_eq!(age_alpha(&[Some(1_100)], 1_000, 600), [255]);
    }

    #[test]
    fn a_short_history_and_missed_volumes_are_said() {
        let c = Coverage {
            requested_s: 3_600,
            from: 0,
            to: 1_200,
            frames: 4,
            missing: 1,
            short_s: 2_100,
        };
        assert_eq!(
            coverage_note(&c),
            ", history covers 25 of 60 min, 1 volume missing"
        );
        let full = Coverage {
            missing: 0,
            short_s: 0,
            ..c
        };
        assert_eq!(coverage_note(&full), "");
    }
}
