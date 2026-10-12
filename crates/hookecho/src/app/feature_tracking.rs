//! Following a feature the analyst picked through the active pane's loop (ROADMAP_NEW R4,
//! 1010.md V14): the gate inspector's "Follow through the loop" starts it, the volumes the pane
//! has decoded are searched off the UI thread (`wxdata::feature_track`), the track is drawn on the
//! map with each fix's time and confidence, and a fix can be put right by hand: "Correct this
//! volume" and a click where the feature really is, after which the track is followed again from
//! there.

use super::*;
use wxdata::feature_track::{FeatureKind, Fix};

type TrackResult = Result<(Vec<Fix>, usize, usize), String>;

/// The track being followed, and what started it.
#[derive(Default)]
pub(crate) struct FeatureTracking {
    /// The last gate the inspector was opened on: its pane and point.
    pub inspected: Option<(usize, [f64; 2])>,
    pub track: Option<FeatureTrack>,
}

pub(crate) struct FeatureTrack {
    pub kind: FeatureKind,
    pub pane: usize,
    pub site: String,
    /// The volume and point the analyst picked.
    pub seed: (DateTime<Utc>, [f64; 2]),
    /// Fixes moved by hand, by volume time.
    pub overrides: Vec<(DateTime<Utc>, [f64; 2])>,
    pub fixes: Vec<Fix>,
    /// Volumes searched, and the loop's volumes left out because they were not decoded.
    pub searched: usize,
    pub not_decoded: usize,
    pub error: Option<String>,
    pub busy: bool,
    /// A hail core waits for the day's melting level before it can be searched.
    pub waiting_levels: bool,
    /// The next map click on the pane moves the current volume's fix there.
    pub correcting: bool,
    rx: Option<std::sync::mpsc::Receiver<TrackResult>>,
}

impl FeatureTrack {
    /// As a case keeps it.
    pub(crate) fn to_case(&self) -> crate::case::CaseFeatureTrack {
        crate::case::CaseFeatureTrack {
            kind: self.kind,
            site: self.site.clone(),
            seed_time: self.seed.0,
            seed: self.seed.1,
            overrides: self.overrides.clone(),
            fixes: self.fixes.clone(),
        }
    }

    /// Reopened from a case on pane `pane`: the saved fixes, not searched again.
    pub(crate) fn from_case(c: &crate::case::CaseFeatureTrack, pane: usize) -> Self {
        FeatureTrack {
            kind: c.kind,
            pane,
            site: c.site.clone(),
            seed: (c.seed_time, c.seed),
            overrides: c.overrides.clone(),
            searched: c.fixes.len(),
            fixes: c.fixes.clone(),
            not_decoded: 0,
            error: None,
            busy: false,
            waiting_levels: false,
            correcting: false,
            rx: None,
        }
    }
}

impl HookEchoApp {
    /// Start following the feature under the inspector's last click: rotation on velocity; a
    /// core, a hail core or a BWER candidate on reflectivity.
    pub(crate) fn start_feature_track(&mut self, kind: FeatureKind) {
        let Some((pane, ll)) = self.feature_tracking.inspected else {
            return;
        };
        let v = &self.views[pane];
        let (Some(site), Some(time)) = (
            v.site.clone(),
            v.timeline.current().and_then(|id| id.date_time()),
        ) else {
            return;
        };
        self.feature_tracking.track = Some(FeatureTrack {
            kind,
            pane,
            site,
            seed: (time, ll),
            overrides: Vec::new(),
            fixes: Vec::new(),
            searched: 0,
            not_decoded: 0,
            error: None,
            busy: false,
            waiting_levels: false,
            correcting: false,
            rx: None,
        });
        self.run_feature_track();
    }

    /// The 0 °C and −20 °C heights above pane `pane`'s radar (m), as the hail estimate takes them:
    /// the live analysis or, on an archived volume, that day's sounding.
    fn hail_levels(&self, pane: usize) -> Option<(f64, f64)> {
        let radar_m = self.views[pane]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map_or(0.0, |s| f64::from(s.elevation_meters));
        self.freezing_for(pane)
            .map(|(h0, hm20)| (h0 - radar_m, hm20 - radar_m))
    }

    /// Search the pane's decoded volumes for the track, off the UI thread.
    fn run_feature_track(&mut self) {
        let Some((kind, pane)) = self
            .feature_tracking
            .track
            .as_ref()
            .map(|t| (t.kind, t.pane))
        else {
            return;
        };
        let levels = self.hail_levels(pane);
        let Some(t) = self.feature_tracking.track.as_mut() else {
            return;
        };
        // A hail core needs the melting level; the window asks for it and runs the search when
        // it comes.
        t.waiting_levels = kind == FeatureKind::Hail && levels.is_none();
        if t.waiting_levels {
            t.error = None;
            return;
        }
        let v = &self.views[t.pane];
        let mut frames: Vec<(DateTime<Utc>, Arc<Scan>)> = Vec::new();
        let mut not_decoded = 0;
        for id in &v.timeline.frames {
            let Some(time) = id.date_time() else { continue };
            match self.scan_cache.peek(id.name()) {
                Some(scan) => frames.push((time, Arc::clone(scan))),
                None => not_decoded += 1,
            }
        }
        frames.sort_by_key(|f| f.0);
        t.not_decoded = not_decoded;
        t.busy = true;
        t.error = None;
        let (seed, overrides) = (t.seed, t.overrides.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        t.rx = Some(rx);
        self.spawner.spawn_blocking(move || {
            let _ = tx.send(follow(kind, &frames, seed, &overrides, levels));
        });
    }

    /// Take a finished search.
    pub(crate) fn poll_feature_track(&mut self) {
        let Some(t) = self.feature_tracking.track.as_mut() else {
            return;
        };
        let Some(rx) = &t.rx else { return };
        if let Ok(res) = rx.try_recv() {
            t.rx = None;
            t.busy = false;
            match res {
                Ok((fixes, searched, unreadable)) => {
                    t.fixes = fixes;
                    t.searched = searched;
                    t.not_decoded += unreadable;
                }
                Err(e) => t.error = Some(e),
            }
        }
    }

    /// A map click while correcting: the pane's current volume's fix goes where it was clicked,
    /// and the track is followed again from it. True when the click was taken.
    pub(crate) fn correct_feature_fix(&mut self, idx: usize, ll: [f64; 2]) -> bool {
        let Some(t) = self.feature_tracking.track.as_mut() else {
            return false;
        };
        if !t.correcting || t.pane != idx {
            return false;
        }
        let Some(time) = self.views[idx]
            .timeline
            .current()
            .and_then(|id| id.date_time())
        else {
            return false;
        };
        t.overrides.retain(|(at, _)| *at != time);
        t.overrides.push((time, ll));
        t.correcting = false;
        self.run_feature_track();
        true
    }

    /// The track window: each fix's time, place, value and confidence, the volume shown marked,
    /// and the correction and clear buttons.
    pub(crate) fn feature_track_window(&mut self, ctx: &egui::Context) {
        self.poll_feature_track();
        if let Some(pane) = self
            .feature_tracking
            .track
            .as_ref()
            .filter(|t| t.waiting_levels)
            .map(|t| t.pane)
        {
            if self.hail_levels(pane).is_some() {
                self.run_feature_track();
            } else {
                // Throttled by the fetch itself; repaint so the answer is picked up.
                self.fetch_freezing_levels(ctx, pane);
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        let tz = self.active_tz();
        let Some(t) = self.feature_tracking.track.as_mut() else {
            return;
        };
        let current = self.views[t.pane]
            .timeline
            .current()
            .and_then(|id| id.date_time());
        let mut open = true;
        let mut clear = false;
        let unit = t.kind.unit();
        egui::Window::new(format!("Following a {}", t.kind.label()))
            .id(egui::Id::new("feature_track"))
            .open(&mut open)
            .default_width(340.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.weak(format!(
                    "{} · picked at {}",
                    t.site,
                    crate::timefmt::fmt_clock(t.seed.0, tz, false)
                ));
                if t.busy || t.waiting_levels {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak(if t.waiting_levels {
                            "fetching the melting level for the hail estimate\u{2026}"
                        } else {
                            "following through the loop\u{2026}"
                        });
                    });
                }
                if let Some(e) = &t.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e);
                }
                if !t.busy && !t.waiting_levels && t.error.is_none() {
                    ui.weak(format!(
                        "Followed through {} of {} volumes searched{}",
                        t.fixes.len(),
                        t.searched,
                        if t.not_decoded > 0 {
                            format!(
                                "; {} more in the loop not decoded yet (play the loop once to \
                                 include them)",
                                t.not_decoded
                            )
                        } else {
                            String::new()
                        }
                    ));
                }
                egui::Grid::new("feature_track_rows")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Time");
                        ui.strong("Where");
                        ui.strong(unit);
                        ui.strong("Confidence");
                        ui.end_row();
                        for f in &t.fixes {
                            let here = current == Some(f.time);
                            let time = crate::timefmt::fmt_clock(f.time, tz, true);
                            if here {
                                ui.strong(format!("\u{25b6} {time}"));
                            } else {
                                ui.label(time);
                            }
                            ui.label(format!("{:.3}, {:.3}", f.ll[1], f.ll[0]));
                            ui.label(f.value.map_or("—".into(), |v| match t.kind {
                                FeatureKind::Rotation => format!("{v:.4}"),
                                FeatureKind::Core | FeatureKind::Hail | FeatureKind::Bwer => {
                                    format!("{v:.0}")
                                }
                            }));
                            ui.label(if f.manual {
                                "by hand".to_string()
                            } else {
                                format!("{:.0} %", f.confidence * 100.0)
                            });
                            ui.end_row();
                        }
                    });
                ui.horizontal(|ui| {
                    let label = if t.correcting {
                        "Click where it is\u{2026}"
                    } else {
                        "Correct this volume"
                    };
                    if ui
                        .selectable_label(t.correcting, label)
                        .on_hover_text(
                            "Then click the map where the feature really is in the volume shown: \
                             that fix is kept as placed and the track is followed again from it.",
                        )
                        .clicked()
                    {
                        t.correcting = !t.correcting;
                    }
                    if ui.button("Clear").clicked() {
                        clear = true;
                    }
                });
                ui.weak(
                    "Confidence is how clearly one feature was followed: its contrast with the \
                     area searched, its strength and how near the motion predicted it. Not a \
                     tornado probability.",
                );
            });
        if clear || !open {
            self.feature_tracking.track = None;
        }
    }

    /// The track on pane `idx`: a line through the fixes, a dot at each sized by confidence
    /// (squares for fixes placed by hand), the volume shown ringed, and the times labelled.
    pub(crate) fn paint_feature_track(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        let Some(t) = &self.feature_tracking.track else {
            return;
        };
        if t.pane != idx || t.fixes.is_empty() {
            return;
        }
        let screen = |ll: [f64; 2]| {
            let (sx, sy) =
                cam.world_to_screen(crate::render::mercator::lonlat_to_world(ll[0], ll[1]), vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        let col = egui::Color32::from_rgb(120, 220, 255);
        let pts: Vec<egui::Pos2> = t.fixes.iter().map(|f| screen(f.ll)).collect();
        painter.add(egui::Shape::line(
            pts.clone(),
            egui::Stroke::new(2.0, col.gamma_multiply(0.8)),
        ));
        let current = self.views[idx]
            .timeline
            .current()
            .and_then(|id| id.date_time());
        let tz = self.active_tz();
        for (f, p) in t.fixes.iter().zip(&pts) {
            let r = 2.5 + 3.0 * f.confidence;
            if f.manual {
                painter.rect_filled(
                    egui::Rect::from_center_size(*p, egui::vec2(8.0, 8.0)),
                    1.0,
                    col,
                );
            } else {
                painter.circle_filled(*p, r, col);
            }
            if current == Some(f.time) {
                painter.circle_stroke(*p, r + 4.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
                painter.text(
                    *p + egui::vec2(10.0, -10.0),
                    egui::Align2::LEFT_BOTTOM,
                    crate::timefmt::fmt_clock(f.time, tz, false),
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}

/// The track through `frames` (time-ordered, decoded volumes) from `seed`, with `overrides`
/// kept as placed: the lowest velocity tilt's LLSD shear for a rotation, the lowest reflectivity
/// tilt for a core, every reflectivity tilt for a hail core (with `levels`, the 0 °C and −20 °C
/// heights above the radar, m) or a BWER candidate. Also how many volumes were searched and how
/// many could not be read.
fn follow(
    kind: FeatureKind,
    frames: &[(DateTime<Utc>, Arc<Scan>)],
    seed: (DateTime<Utc>, [f64; 2]),
    overrides: &[(DateTime<Utc>, [f64; 2])],
    levels: Option<(f64, f64)>,
) -> TrackResult {
    use wxdata::feature_track as ft;
    enum Sampled {
        Shear(wxdata::azshear::AzShearField),
        Refl(wxdata::level2::BinnedSweep),
        Volume(Vec<wxdata::level2::BinnedSweep>),
    }
    let (h0, hm20) = match (kind, levels) {
        (FeatureKind::Hail, None) => return Err("No melting level for the hail estimate".into()),
        (_, l) => l.unwrap_or((0.0, 0.0)),
    };
    let mut times = Vec::new();
    let mut sampled = Vec::new();
    let mut unreadable = 0;
    for (time, scan) in frames {
        let s = match kind {
            FeatureKind::Rotation => {
                wxdata::level2::bin_scan_opts_recorded(scan, Moment::Velocity, 0, true)
                    .map(|v| Sampled::Shear(wxdata::azshear::llsd(&v, &Default::default())))
            }
            FeatureKind::Core => {
                wxdata::level2::bin_scan_opts_recorded(scan, Moment::Reflectivity, 0, false)
                    .map(Sampled::Refl)
            }
            // The column needs every tilt.
            FeatureKind::Hail | FeatureKind::Bwer => {
                let tilts: Vec<_> = (0..wxdata::level2::elevation_angles(scan).len())
                    .filter_map(|tilt| {
                        wxdata::level2::bin_scan_opts_recorded(
                            scan,
                            Moment::Reflectivity,
                            tilt,
                            false,
                        )
                        .ok()
                    })
                    .collect();
                if tilts.len() >= 2 {
                    Ok(Sampled::Volume(tilts))
                } else {
                    Err(anyhow::anyhow!("fewer than two reflectivity tilts"))
                }
            }
        };
        match s {
            Ok(s) => {
                times.push(*time);
                sampled.push(s);
            }
            Err(_) => unreadable += 1,
        }
    }
    if times.is_empty() {
        return Err("No decoded volume in the loop has the moment to follow".into());
    }
    let seed_i = (0..times.len())
        .min_by_key(|&i| (times[i] - seed.0).num_seconds().abs())
        .unwrap_or(0);
    let by_index: Vec<(usize, [f64; 2])> = overrides
        .iter()
        .filter_map(|(at, ll)| times.iter().position(|t| t == at).map(|i| (i, *ll)))
        .collect();
    let fixes = ft::track(
        &times,
        (seed_i, seed.1),
        &by_index,
        ft::TrackParams::for_kind(kind),
        |i, ll, r| match &sampled[i] {
            Sampled::Shear(f) => ft::rotation_peak(f, ll, r),
            Sampled::Refl(s) => ft::core_peak(s, ll, r),
            Sampled::Volume(v) if kind == FeatureKind::Hail => ft::hail_peak(v, ll, r, h0, hm20),
            Sampled::Volume(v) => ft::bwer_peak(v, ll, r),
        },
    );
    Ok((
        fixes.into_iter().flatten().collect(),
        times.len(),
        unreadable,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// A followed track saved with a case reopens with its fixes and hand fixes as they were,
    /// idle, on the pane given, and saves back the same.
    #[test]
    fn a_followed_track_round_trips_through_a_case() {
        let t0 = Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 29).unwrap();
        let fix = |m: i64, lon: f64, manual: bool| Fix {
            time: t0 + chrono::Duration::minutes(m),
            ll: [lon, 35.3],
            value: Some(0.03),
            confidence: if manual { 1.0 } else { 0.8 },
            manual,
        };
        let saved = crate::case::CaseFeatureTrack {
            kind: FeatureKind::Rotation,
            site: "KTLX".into(),
            seed_time: t0,
            seed: [-97.52, 35.31],
            overrides: vec![(t0 + chrono::Duration::minutes(4), [-97.5, 35.3])],
            fixes: vec![
                fix(0, -97.52, false),
                fix(4, -97.5, true),
                fix(8, -97.47, false),
            ],
        };
        let json = serde_json::to_string(&saved).unwrap();
        let back = FeatureTrack::from_case(&serde_json::from_str(&json).unwrap(), 1);
        assert_eq!(back.pane, 1);
        assert_eq!(back.searched, 3);
        assert!(!back.busy && !back.correcting && back.rx.is_none() && back.error.is_none());
        assert_eq!(back.to_case(), saved);
    }

    /// The app's search on the Moore volumes (the archive, cached by wxdata's own test under
    /// `target/tmp/feature-track`): picked near Moore at 20:12 it follows the mesocyclone through
    /// the loop; moving one fix by hand keeps it there, marked, and the rest still follow.
    #[tokio::test]
    #[ignore = "network: the KTLX archive"]
    async fn live_the_apps_search_follows_moore_and_keeps_a_hand_fix() {
        let date = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
        let from = Utc.with_ymd_and_hms(2013, 5, 20, 19, 50, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2013, 5, 20, 20, 35, 0).unwrap();
        let cache =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/feature-track");
        std::fs::create_dir_all(&cache).unwrap();
        let mut frames = Vec::new();
        for id in wxdata::level2::list_volumes("KTLX", date).await.unwrap() {
            let Some(t) = id.date_time().filter(|t| *t >= from && *t <= to) else {
                continue;
            };
            let scan = wxdata::level2::download_scan(id, Some(cache.clone()))
                .await
                .unwrap();
            frames.push((t, Arc::new(scan)));
        }
        let seed = (
            Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 29).unwrap(),
            [-97.52, 35.31],
        );
        let (fixes, searched, unreadable) =
            follow(FeatureKind::Rotation, &frames, seed, &[], None).unwrap();
        eprintln!(
            "{} fixes of {searched} searched, {unreadable} unreadable",
            fixes.len()
        );
        assert!(fixes.len() >= 8 && unreadable == 0);
        let moved_time = fixes[2].time;
        let moved = [fixes[2].ll[0] + 0.02, fixes[2].ll[1]];
        let (again, ..) = follow(
            FeatureKind::Rotation,
            &frames,
            seed,
            &[(moved_time, moved)],
            None,
        )
        .unwrap();
        let hand = again.iter().find(|f| f.time == moved_time).unwrap();
        assert!(hand.manual && hand.ll == moved);
        assert!(again.len() >= 8, "{}", again.len());
    }
}
