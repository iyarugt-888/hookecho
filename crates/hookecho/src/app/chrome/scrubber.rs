//! The scrubber pill: the transport, the clock, and a drawn time track, floating over the map's
//! bottom edge.
//!
//! The track is drawn rather than being an `egui::Slider` because it carries more than a value:
//! volume ticks, hour labels, the forecast tail and the live loop window all have to read off the
//! same axis.

use super::*;
use crate::ui::a11y::Named as _;
use crate::ui::wsv3;
use egui::{Align, Layout};

impl HookEchoApp {
    /// theme_plan.md §7: dispatches to whichever `TimelineStyle` the user picked. Every style
    /// drives the same `crate::timeline::Timeline` state (`playhead`/`following`/`playing`) —
    /// only the paint/interaction surface differs.
    pub(crate) fn scrubber(&mut self, ctx: &egui::Context) {
        match self.settings.timeline_style {
            crate::settings::TimelineStyle::Default => self.scrubber_default(ctx),
            crate::settings::TimelineStyle::Wsv3 => self.scrubber_wsv3_style(ctx),
            crate::settings::TimelineStyle::Compact => self.scrubber_compact_style(ctx),
        }
    }

    fn scrubber_default(&mut self, ctx: &egui::Context) {
        crate::prof_scope!("scrubber");
        use egui_phosphor::regular as ph;
        let accent = self.chrome_accent();
        let tz = self.active_tz();
        // The site's own newest known frame, not whatever the displayed volume happens to be: a
        // rolling live loop deliberately keeps showing its playhead frame while a genuinely new
        // head is appended to the timeline behind the scenes (see the `DataMsg::Volume` handler's
        // `looping && new_head` case) — reading `views[active].volume.time` here instead used to
        // flip the badge to "Stale" and the age to hours old every time the loop's animation
        // wasn't on the newest frame, even though the feed itself was current the whole time.
        let newest_time = self.views[self.active]
            .timeline
            .newest()
            .and_then(|id| id.date_time());
        let fresh =
            newest_time.is_some_and(|t| (chrono::Utc::now() - t).num_seconds() < RADAR_FRESH_SECS);
        // Site and data age used to live in the docked status bar; the clock belongs with the clock.
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "no site".to_string());
        let age = newest_time.map(|t| {
            let secs = (Utc::now() - t).num_seconds().max(0);
            format!("Scan {} ago", humanize(secs))
        });
        let loading = self.views[self.active].loading;
        // Which mechanism is actually feeding the pane: the sweep-by-sweep chunk stream, or the
        // interval poller it falls back to when the stream can't run.
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(v, _, _, _)| *v == self.active);
        // Read before `t` below takes its mutable borrow of the same view's `timeline` field.
        let live_progress = self.views[self.active].live_progress;
        let show_live_indicator = self.settings.live_scan_indicator;
        // TDWR and DWD radars publish no archive, so their timeline is empty by design and stays
        // live. Saying "(no volumes)" there reads as a failed fetch while a live volume is on
        // screen; only a site that HAS an archive can be genuinely missing one.
        let archived = self.views[self.active]
            .site
            .as_deref()
            .is_some_and(wxdata::sites::is_nexrad);
        let mut go_head = false;
        // Offline chase packs are a browser-only idea: on desktop the volumes are already on disk.
        #[cfg(target_arch = "wasm32")]
        let mut save_pack = false;
        #[cfg(target_arch = "wasm32")]
        let mut load_pack: Option<crate::webcache::Pack> = None;
        #[cfg(target_arch = "wasm32")]
        let (packs, pack_status) = (self.packs(), self.pack_status());
        // Soonest rain arrival and the DVR buffer depth ride the pill: both are about time, and
        // both used to sit in an always-on chip in the opposite corner.
        let rain = self
            .rain_eta
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, min)| format!("\u{1f327} {name} ~{min:.0} min"));
        let dvr = self.dvr_depth();
        // Edited through a local so the pill closure keeps its single `&mut self.views` borrow.
        let mut loop_frames = self.settings.live_loop_frames;
        let narrow = self.chrome_rect.width() < 600.0;
        let compact_live = narrow;
        // A finger, not a pointer: the narrow strip was drawn for a narrow desktop window, with 28 pt
        // buttons and a 3 pt track, far under the 48 dp a thumb needs. `phone` is the touch layout
        // specifically, so a narrow desktop window keeps the small strip.
        let phone = crate::platform::phone_layout();
        // Where the scrubber lands, for the tour's spotlight (same reason: no `self` in there).
        let mut scrub_rect = None;
        // Wide enough for the track to be worth scrubbing, never so wide it spans a 4K map — and
        // never wider than the screen, which on a phone the 420 pt floor would otherwise be.
        let width = (self.chrome_rect.width() - 160.0)
            .clamp(420.0, if narrow { 420.0 } else { 760.0 })
            .min(self.chrome_rect.width() - 16.0);
        // The phone's pill drops the two extras: the readouts fit a desktop row, not a 400 pt one,
        // and rain arrival has its own chip lane.
        let (dvr, rain) = if narrow { (0, None) } else { (dvr, rain) };
        let live_window = self.views[self.active].timeline.live_window;
        egui::Area::new(egui::Id::new("scrubber"))
            .constrain_to(self.chrome_rect)
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, if narrow { crate::ui::style::LANE_BOTTOM_CHIP - self.phone_nav_h() } else { -24.0 }),
            )
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, self.chrome_alpha(252))
                    .corner_radius(self.chrome_corner(crate::ui::style::RADIUS_LG))
                    .inner_margin(egui::Margin::symmetric(
                        if phone { 12 } else if compact_live { 10 } else { 12 },
                        if phone { 6 } else if compact_live { 4 } else { 9 },
                    ))
                    .show(ui, |ui| {
                // The frame's own margins come off the width, or the strip runs past the edge.
                ui.set_width(if phone { width - 24.0 } else { width });
                if compact_live {
                    ui.spacing_mut().item_spacing = egui::vec2(if phone { 2.0 } else { 4.0 }, 0.0);
                }
                let t = &mut self.views[self.active].timeline;
                if t.slot_count() > 0 {
                    scrub_rect = Some(track(ui, t, tz, accent, live_window, compact_live));
                }
                if !narrow { ui.add_space(4.0); ui.separator(); }
                let row = ui.horizontal(|ui| {
                    // The phone says the site in its search pill; a second copy here is 60 pt of
                    // a 400 pt row spent saying it twice, and the clock loses that argument.
                    if !narrow {
                        ui.label(
                            egui::RichText::new(&site)
                                .size(crate::ui::style::FONT_BASE)
                                .strong()
                                .color(egui::Color32::from_gray(238)),
                        );
                    }
                    let btn = |ui: &mut egui::Ui, glyph: &str, on: bool, name: &str| {
                        let primary = name == "Play" || name == "Pause";
                        let fg = if on {
                            accent
                        } else {
                            egui::Color32::from_gray(225)
                        };
                        ui.add(
                            egui::Button::new(egui::RichText::new(glyph).size(if primary && !narrow { 26.0 } else if phone { 22.0 } else { 16.0 }).color(fg))
                                .min_size(egui::vec2(
                                    if phone { 44.0 } else if narrow { 28.0 } else if primary { 48.0 } else { 32.0 },
                                    if phone { 44.0 } else if narrow { 28.0 } else if primary { 48.0 } else { 32.0 },
                                ))
                                .fill(if primary && !narrow { accent.gamma_multiply(0.28) } else { egui::Color32::TRANSPARENT })
                                .corner_radius(24.0)
                                .stroke(if primary && !narrow { egui::Stroke::new(1.0, accent) } else { egui::Stroke::NONE }),
                        )
                        .named_toggle(name, on)
                        .clicked()
                    };
                    if btn(ui, ph::SKIP_BACK, false, "Previous frame") {
                        t.step(-1);
                    }
                    let playing = t.playing;
                    if btn(
                        ui,
                        if playing { ph::PAUSE } else { ph::PLAY },
                        playing,
                        if playing { "Pause" } else { "Play" },
                    ) {
                        t.toggle_play();
                    }
                    if btn(ui, ph::SKIP_FORWARD, false, "Next frame") {
                        t.step(1);
                    }
                    let clock_size = egui::vec2(if phone { 92.0 } else if narrow { 100.0 } else { (ui.available_width() - 210.0).max(170.0) }, if phone { 44.0 } else if narrow { 28.0 } else { 54.0 });
                    ui.allocate_ui_with_layout(
                        clock_size,
                        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center),
                        |ui| {
                    ui.set_min_size(clock_size);
                    // The clock and status readouts share the broadcast row beneath the track.
                    let observed = t.frames.len();
                    if observed == 0 {
                        ui.weak(if t.listing {
                            "listing volumes\u{2026}"
                        } else if archived {
                            "(no volumes)"
                        } else {
                            "live only"
                        });
                    } else {
                        let readout = match t.forecast_hour() {
                            Some(h) => format!("F+{h}h"),
                            // The transport, the badge, the clock and the age share one row, and
                            // on a phone that leaves the clock about a hundred points —
                            // "5:10:35 PM CDT" ran off the edge and under the age readout. When
                            // the room is not there the seconds and the zone go first: a phone's
                            // zone is the one it is standing in.
                            None => t
                                .current()
                                .and_then(|id| id.date_time())
                                .map(|d| match tz {
                                    Some(tz) if narrow || ui.available_width() < 190.0 => {
                                        d.with_timezone(&tz).format("%-I:%M %p").to_string()
                                    }
                                    _ => crate::timefmt::fmt_clock(d, tz, false),
                                })
                                .unwrap_or_default(),
                        };
                        ui.add_sized(clock_size, egui::Label::new(
                            egui::RichText::new(readout)
                                .size(if phone { 17.0 } else if narrow { 15.0 } else { 22.0 })
                                .strong()
                                .color(egui::Color32::from_gray(238)),
                        ));
                    }
                    });
                    // Live / archive badge: click to re-pin to the newest volume.
                    //
                    // Pinned to live but the newest volume is old means the site's feed has
                    // stopped, not that playback drifted — there is nothing newer to jump to.
                    // The colour said so already; saying "LIVE" over a volume hours old did not.
                    let (col, text, hint) = if t.following && fresh {
                        (
                            mobile::OMEGA_GREEN,
                            "Live".to_string(),
                            if streaming {
                                match live_progress {
                                    // Scan-in-progress detail between sweep merges — the same
                                    // metadata the vendored chunk mapper already computes from
                                    // the VCP, just surfaced rather than thrown away.
                                    Some(p) => format!(
                                        "Following the newest volume, sweep by sweep (live \
                                         stream). Sweep {}/{} at {:.1}\u{b0}, chunk {}/{}.",
                                        p.elevation_number,
                                        p.total_elevations,
                                        p.elevation_angle_deg,
                                        p.chunk_index,
                                        p.chunks_in_sweep,
                                    ),
                                    None => "Following the newest volume, sweep by sweep (live \
                                             stream)."
                                        .to_string(),
                                }
                            } else {
                                "Following the newest volume, polling for new ones (no live \
                                 stream — this site has none, or it is retrying)."
                                    .to_string()
                            },
                        )
                    } else if t.following {
                        (
                            egui::Color32::from_rgb(220, 180, 0),
                            "Stale".to_string(),
                            "Following the newest volume, but this site has not produced one \
                             recently — its feed has stopped. The age next to the clock is how \
                             far behind it is."
                                .to_string(),
                        )
                    } else {
                        (
                            egui::Color32::from_gray(150),
                            if narrow { "Archive".to_string() } else { format!("Archive {}", t.date.format("%m/%d")) },
                            "Scrubbed to an archive day. Click to jump back to live.".to_string(),
                        )
                    };
                    // The animated ring: "still connected, here's roughly how far through this
                    // volume the radar is" at a glance, without reading the badge's own hover
                    // text. Only while the same condition already turns the badge green — a ring
                    // spinning next to a stale or archived badge would say the opposite of what's
                    // actually happening.
                    if show_live_indicator && t.following && fresh && streaming {
                        if let Some(p) = live_progress {
                            live_progress_ring(ui, p, accent);
                            ui.add_space(2.0);
                        }
                    }
                    let badge = ui.add(
                        egui::Button::new(
                            egui::RichText::new(format!("● {text}"))
                                .size(if phone { 14.0 } else { 12.0 })
                                .strong()
                                .color(col),
                        )
                        .min_size(if phone { egui::vec2(0.0, 40.0) } else { egui::Vec2::ZERO })
                        .fill(egui::Color32::TRANSPARENT)
                        .corner_radius(9.0),
                    )
                    .named(&hint);
                    if badge.clicked() {
                        go_head = true;
                    }
                    // Right-click the badge, or use the calendar button, for archive and playback
                    // settings. Both open this one existing menu.
                    let timeline_popup = egui::Popup::default_response_id(&badge);
                    egui::Popup::context_menu(&badge)
                        .anchor(&badge)
                        .align(egui::RectAlign::TOP_START)
                        // `context_menu`'s default (`CloseOnClick`) closes this on ANY click,
                        // inside the menu or out — fine for a one-shot pick, but this menu holds
                        // multi-step controls (the calendar toggle below, the fps slider) that
                        // need several clicks in a row without the whole thing vanishing after
                        // the first one.
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(240.0);
                        if dvr > 1 {
                            ui.label(
                                egui::RichText::new(format!("\u{27f2} {dvr}"))
                                    .size(crate::ui::style::FONT_SM)
                                    .color(egui::Color32::from_gray(150)),
                            )
                            .on_hover_text("Frames buffered in memory for instant replay (R)");
                        }
                        if let Some(r) = &rain {
                            ui.label(
                                egui::RichText::new(r)
                                    .size(crate::ui::style::FONT_SM)
                                    .color(egui::Color32::from_rgb(110, 180, 240)),
                            )
                            .on_hover_text(
                                "Estimated from storm motion \u{2014} rough for backbuilding storms",
                            );
                        }

                            ui.horizontal(|ui| {
                                ui.label("Date:");
                                if ui
                                    .button(egui_phosphor::regular::CARET_LEFT)
                                    .named("Previous day")
                                    .clicked()
                                {
                                    if let Some(d) = t.date.pred_opt() {
                                        seek_to_day(t, &site, d);
                                    }
                                }
                                // The carets are still the fastest way to step a day, but they
                                // were the *only* way: reaching a storm from years back meant
                                // thousands of clicks. The archive runs to June 1991.
                                if let Some(d) = archive_day_input(ui, t.date) {
                                    seek_to_day(t, &site, d);
                                }
                                let is_today = t.date >= chrono::Utc::now().date_naive();
                                if ui
                                    .add_enabled(
                                        !is_today,
                                        egui::Button::new(egui_phosphor::regular::CARET_RIGHT),
                                    )
                                    .named("Next day")
                                    .clicked()
                                {
                                    if let Some(d) = t.date.succ_opt() {
                                        seek_to_day(t, &site, d);
                                    }
                                }
                            });
                            if let Some(d) = archive_day_calendar(ui, t.date) {
                                seek_to_day(t, &site, d);
                            }
                            // A typed jump to an exact hour:minute, next to the day it applies
                            // to — the track below is drag-precise, but "3:47Z" specifically is
                            // faster typed than found by eye. Shows the playhead's own time so
                            // the fields already read right before anyone touches them; editing
                            // either jumps straight to the nearest volume on `t.date`.
                            if !t.frames.is_empty() {
                                use chrono::Timelike;
                                let now = t
                                    .current()
                                    .and_then(|id| id.date_time())
                                    .unwrap_or_else(Utc::now);
                                let (mut hour, mut minute) = (now.hour(), now.minute());
                                ui.horizontal(|ui| {
                                    ui.label("Time (UTC):");
                                    let hr = ui.add(
                                        egui::DragValue::new(&mut hour)
                                            .range(0..=23)
                                            .suffix("h"),
                                    );
                                    let mr = ui.add(
                                        egui::DragValue::new(&mut minute)
                                            .range(0..=59)
                                            .suffix("m"),
                                    );
                                    if hr.changed() || mr.changed() {
                                        t.seek_to_time_of_day(hour, minute);
                                    }
                                })
                                .response
                                .on_hover_text(
                                    "Jump to the volume nearest this time on the selected day",
                                );
                            }
                            ui.horizontal(|ui| {
                                if ui.button("⏮").on_hover_text("First frame").clicked() {
                                    t.go_begin();
                                }
                                ui.checkbox(&mut t.loop_enabled, "Loop");
                            });
                            ui.add(
                                egui::Slider::new(&mut t.speed, 1.0..=15.0)
                                    .suffix(" fps")
                                    .show_value(true),
                            );
                            ui.add(
                                egui::DragValue::new(&mut loop_frames)
                                    .range(2..=30)
                                    .suffix(" frames"),
                            )
                            .on_hover_text(
                                "How many of the newest volumes ▶ cycles through when live",
                            );
                            #[cfg(target_arch = "wasm32")]
                            {
                                ui.separator();
                                if ui
                                    .button("Save this loop offline")
                                    .on_hover_text(
                                        "Keep this loop's volumes in the browser so it plays \
                                         with no signal. Archived volumes only \u{2014} the live \
                                         head is still being written.",
                                    )
                                    .clicked()
                                {
                                    save_pack = true;
                                }
                                for p in &packs {
                                    ui.horizontal(|ui| {
                                        if ui.button(p.label()).clicked() {
                                            load_pack = Some(p.clone());
                                        }
                                        if ui.small_button("\u{d7}").on_hover_text("Delete").clicked()
                                        {
                                            load_pack = None;
                                            crate::webcache::spawn_remove(p.key());
                                        }
                                    });
                                }
                                if let Some(msg) = &pack_status {
                                    ui.weak(msg);
                                }
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let archive = ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(ph::DOTS_THREE)
                                        .size(18.0)
                                        .color(egui::Color32::from_gray(225)),
                                )
                                .min_size(egui::vec2(
                                    if compact_live { 24.0 } else { 30.0 },
                                    if compact_live { 24.0 } else { 30.0 },
                                ))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                            )
                            .named("Archive and playback settings");
                        if archive.clicked() {
                            egui::Popup::toggle_id(ui.ctx(), timeline_popup);
                        }
                        // "15.1y ago" next to an ARCHIVE 04/27 badge is the same fact twice, and
                        // on a phone the two of them plus the clock overrun the row and draw on
                        // top of each other. Scrubbed to the archive, the badge already carries
                        // the date; the age only earns its place while the timeline is live.
                        let age = match (&age, t.following) {
                            _ if narrow => &None,
                            (Some(_), false) if ui.available_width() < 150.0 => &None,
                            _ => &age,
                        };
                        if let Some(age) = age {
                            ui.label(
                                egui::RichText::new(age)
                                    .size(crate::ui::style::FONT_SM)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        } else if loading && !narrow {
                            ui.label(
                                egui::RichText::new("loading\u{2026}")
                                    .size(crate::ui::style::FONT_SM)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }
                    });
                });
                if let Some(rect) = &mut scrub_rect {
                    *rect = rect.union(row.response.rect);
                }
                // The tilt-progress bar: how far the current live volume has scanned, as a
                // fraction of its total elevations — the linear complement to the ring's glance
                // read, precise enough to actually name the tilt in its hover text.
                if show_live_indicator && t.following && fresh && streaming {
                    if let Some(p) = live_progress {
                        ui.add_space(3.0);
                        let fraction = if p.total_elevations > 0 {
                            p.elevation_number as f32 / p.total_elevations as f32
                        } else {
                            0.0
                        };
                        let bar = ui
                            .add(
                                egui::ProgressBar::new(fraction)
                                    .desired_height(4.0)
                                    .fill(accent)
                                    .corner_radius(2.0),
                            )
                            .on_hover_text(format!(
                                "Tilt {}/{} at {:.1}\u{b0} \u{2014} chunk {}/{} of the current \
                                 sweep",
                                p.elevation_number,
                                p.total_elevations,
                                p.elevation_angle_deg,
                                p.chunk_index,
                                p.chunks_in_sweep,
                            ));
                        if let Some(rect) = &mut scrub_rect {
                            *rect = rect.union(bar.rect);
                        }
                    }
                }
                if narrow {
                    let status = if !t.following {
                        t.date.format("%Y-%m-%d").to_string()
                    } else {
                        age.clone().unwrap_or_else(|| if loading { "Loading radar…".into() } else { String::new() })
                    };
                    let response = ui.small(status);
                    if let Some(rect) = &mut scrub_rect {
                        *rect = rect.union(response.rect);
                    }
                }
                });
            });
        self.settings.live_loop_frames = loop_frames;
        #[cfg(target_arch = "wasm32")]
        {
            if save_pack {
                self.save_offline_pack(ctx);
            }
            if let Some(p) = load_pack {
                self.load_offline_pack(&p);
            }
        }
        if let Some(r) = scrub_rect {
            // The scrubber swallows two-finger gestures on the phone like any other surface.
            self.mobile_occlusion.push(r);
        }
        self.tour_anchors.timeline = scrub_rect;
        if go_head {
            self.views[self.active].timeline.go_head();
        }
    }

    /// theme_plan.md §7's WSV3-styled timeline: an explicit transport row (skip-to-start/rewind/
    /// pause-play/fast-forward/skip-to-end), a loop-length control (`Settings.live_loop_frames`,
    /// the same setting the default style's popup menu already edits — this is a second surface
    /// for the same value, not a second value), and a plain `egui::Slider` below rather than the
    /// default style's hand-painted track. Every control drives the same `Timeline` fields the
    /// default style does (`playhead`/`playing`/`following`), via the exact same three-line idiom
    /// `track()`'s own drag handler uses (see that function, below).
    fn scrubber_wsv3_style(&mut self, ctx: &egui::Context) {
        use egui_phosphor::regular as ph;
        let accent = wsv3::WSV3_BLUE;
        let tz = self.active_tz();
        let newest_time = self.views[self.active]
            .timeline
            .newest()
            .and_then(|id| id.date_time());
        let fresh =
            newest_time.is_some_and(|t| (chrono::Utc::now() - t).num_seconds() < RADAR_FRESH_SECS);
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "no site".to_string());
        let mut loop_frames = self.settings.live_loop_frames;
        let narrow = self.chrome_rect.width() < 600.0;
        let width = (self.chrome_rect.width() - 160.0)
            .clamp(420.0, 820.0)
            .min(self.chrome_rect.width() - 16.0);
        let mut go_head = false;
        let mut scrub_rect = None;

        egui::Area::new(egui::Id::new("scrubber_wsv3"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(wsv3::STATUS_BG)
                    .corner_radius(6.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_black_alpha(170)))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
                        scrub_rect = Some(rect);
                        let t = &mut self.views[self.active].timeline;
                        let slots = t.slot_count();
                        ui.horizontal(|ui| {
                            let icon = |ui: &mut egui::Ui, glyph: &str, name: &str| {
                                ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(glyph).size(15.0).color(accent),
                                    )
                                    .min_size(egui::vec2(26.0, 26.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .named(name)
                                .clicked()
                            };
                            if icon(ui, ph::SKIP_BACK, "Jump to start") {
                                t.go_begin();
                            }
                            if icon(ui, ph::REWIND, "Previous frame") {
                                t.step(-1);
                            }
                            let playing = t.playing;
                            if icon(
                                ui,
                                if playing { ph::PAUSE } else { ph::PLAY },
                                if playing { "Pause" } else { "Play" },
                            ) {
                                t.toggle_play();
                            }
                            if icon(ui, ph::FAST_FORWARD, "Next frame") {
                                t.step(1);
                            }
                            if icon(ui, ph::SKIP_FORWARD, "Jump to live") {
                                go_head = true;
                            }
                            ui.separator();
                            let valid = t
                                .current()
                                .and_then(|id| id.date_time())
                                .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                                .unwrap_or_default();
                            ui.label(
                                egui::RichText::new(if valid.is_empty() {
                                    site.clone()
                                } else {
                                    valid
                                })
                                .size(13.0)
                                .strong()
                                .color(wsv3::INK),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (col, text) = if t.following && fresh {
                                    (mobile::OMEGA_GREEN, "LIVE")
                                } else if t.following {
                                    (egui::Color32::from_rgb(220, 180, 0), "STALE")
                                } else {
                                    (egui::Color32::from_gray(150), "ARCHIVE")
                                };
                                ui.label(
                                    egui::RichText::new(format!("\u{25cf} {text}"))
                                        .size(11.0)
                                        .strong()
                                        .color(col),
                                );
                                if !narrow {
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new("LOOP")
                                            .size(9.0)
                                            .color(wsv3::STATUS_FG.gamma_multiply(0.7)),
                                    );
                                    egui::ComboBox::from_id_salt("wsv3_timeline_loop_frames")
                                        .selected_text(format!("{loop_frames}"))
                                        .width(48.0)
                                        .show_ui(ui, |ui| {
                                            for n in [6usize, 12, 24, 48] {
                                                ui.selectable_value(
                                                    &mut loop_frames,
                                                    n,
                                                    format!("{n}"),
                                                );
                                            }
                                        });
                                }
                            });
                        });
                        if slots > 0 {
                            let mut idx = t.playhead.min(slots.saturating_sub(1));
                            let observed = t.frames.len();
                            if ui
                                .add(
                                    egui::Slider::new(&mut idx, 0..=slots.saturating_sub(1))
                                        .show_value(false),
                                )
                                .changed()
                            {
                                t.playhead = idx;
                                t.playing = false;
                                t.following = idx + 1 == observed;
                            }
                        }
                    });
            });
        self.settings.live_loop_frames = loop_frames;
        if let Some(r) = scrub_rect {
            self.mobile_occlusion.push(r);
        }
        self.tour_anchors.timeline = scrub_rect;
        if go_head {
            self.views[self.active].timeline.go_head();
        }
    }

    /// theme_plan.md §7's compact timeline: the same hand-drawn scrub track the default style
    /// uses (`track()`, below — already has a `compact` painting mode), in a slim single row with
    /// only prev/play/next and the live badge — no popup menu, no rain-ETA/DVR-depth extras. The
    /// general "compact timeline" archetype (GR2Analyst/WeatherFront-style tools), not a
    /// pixel-exact copy of either — see `TimelineStyle::Compact`'s own doc comment for why.
    fn scrubber_compact_style(&mut self, ctx: &egui::Context) {
        use egui_phosphor::regular as ph;
        let accent = crate::theme::accent(self.settings.theme);
        let tz = self.active_tz();
        let newest_time = self.views[self.active]
            .timeline
            .newest()
            .and_then(|id| id.date_time());
        let fresh =
            newest_time.is_some_and(|t| (chrono::Utc::now() - t).num_seconds() < RADAR_FRESH_SECS);
        let live_window = self.views[self.active].timeline.live_window;
        let width = (self.chrome_rect.width() - 160.0)
            .clamp(320.0, 640.0)
            .min(self.chrome_rect.width() - 16.0);
        let mut go_head = false;
        let mut scrub_rect = None;

        egui::Area::new(egui::Id::new("scrubber_compact"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, 220)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let icon = |ui: &mut egui::Ui, glyph: &str, name: &str| {
                                ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(glyph).size(13.0).color(accent),
                                    )
                                    .min_size(egui::vec2(22.0, 22.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .named(name)
                                .clicked()
                            };
                            let t = &mut self.views[self.active].timeline;
                            if icon(ui, ph::SKIP_BACK, "Previous frame") {
                                t.step(-1);
                            }
                            let playing = t.playing;
                            if icon(
                                ui,
                                if playing { ph::PAUSE } else { ph::PLAY },
                                if playing { "Pause" } else { "Play" },
                            ) {
                                t.toggle_play();
                            }
                            if icon(ui, ph::SKIP_FORWARD, "Next frame") {
                                t.step(1);
                            }
                            if t.slot_count() > 0 {
                                scrub_rect = Some(track(ui, t, tz, accent, live_window, true));
                            }
                            let col = if t.following && fresh {
                                mobile::OMEGA_GREEN
                            } else if t.following {
                                egui::Color32::from_rgb(220, 180, 0)
                            } else {
                                egui::Color32::from_gray(150)
                            };
                            let badge = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("\u{25cf}").size(11.0).color(col),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .named(if t.following && fresh {
                                    "Live"
                                } else if t.following {
                                    "Stale"
                                } else {
                                    "Archive — click to jump back to live"
                                });
                            if badge.clicked() {
                                go_head = true;
                            }
                        });
                    });
            });
        if let Some(r) = scrub_rect {
            self.mobile_occlusion.push(r);
        }
        self.tour_anchors.timeline = scrub_rect;
        if go_head {
            self.views[self.active].timeline.go_head();
        }
    }
}

/// Jump the timeline to a different UTC day, landing near the same time of day it was already
/// showing (noon if nothing was on screen yet) — shared by the day-step carets, the typed date
/// field, and the calendar picker so all three land correctly instead of only overwriting `t.date`
/// and leaving `t.frames`/`t.playhead` to mean nothing on the new day until the next listing
/// happens to land on an in-range index by coincidence. That was the bug behind archived scans
/// "not always" loading from the calendar: a plain `t.date = d` never cleared the old day's stale
/// playhead index, so a jump to a day with fewer frames than that index could silently show
/// nothing, and even a jump that stayed in range showed whatever arbitrary moment the old index
/// happened to land on rather than the day the user actually asked to see. A URL/permalink jump
/// never had this problem because it already went through `seek_to_valid_time`.
pub(super) fn seek_to_day(
    t: &mut crate::timeline::Timeline,
    site: &str,
    new_date: chrono::NaiveDate,
) {
    let today = chrono::Utc::now().date_naive();
    // Clamp rather than trust the input: neither a calendar widget, a typed string, nor a
    // one-day step knows where the archive starts or that the future is empty.
    let new_date = new_date.clamp(wxdata::level2::ARCHIVE_START, today);
    if new_date >= today {
        t.follow_day(new_date);
        return;
    }
    let time_of_day = t
        .current()
        .and_then(|id| id.date_time())
        .map(|dt| dt.time())
        .unwrap_or_else(|| chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    t.seek_to_valid_time(site, new_date.and_time(time_of_day).and_utc());
}

#[cfg(test)]
mod seek_to_day_tests {
    use super::seek_to_day;
    use crate::timeline::Timeline;
    use wxdata::level2::Identifier;

    fn frames_at(date: chrono::NaiveDate, times: &[(u32, u32)]) -> Vec<Identifier> {
        times
            .iter()
            .map(|&(h, m)| {
                Identifier::new(format!("KTLX{}_{h:02}{m:02}00_V06", date.format("%Y%m%d")))
            })
            .collect()
    }

    /// The bug this function fixes: `t.date = d` alone left the old day's numeric playhead index
    /// in place, so a jump to a differently-shaped day showed whatever arbitrary moment that index
    /// happened to mean there — not the day, let alone the time, the user actually picked. A
    /// correct jump lands near the *same time of day* regardless of how many frames either day has.
    #[test]
    fn the_new_days_frame_is_picked_by_time_of_day_not_by_reusing_the_old_index() {
        let day1 = chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap();
        let day2 = chrono::NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let mut t = Timeline::default();
        t.date = day1;
        // Three volumes on day1; the playhead sits on the last one, 23:55.
        t.set_frames(
            frames_at(day1, &[(0, 0), (12, 0), (23, 55)]),
            ("KTLX".into(), day1),
        );
        t.playhead = 2;

        seek_to_day(&mut t, "KTLX", day2);
        assert_eq!(t.date, day2, "the day itself changed immediately");

        // day2's listing lands with far more frames than day1 had (a full day, 5 minutes apart) —
        // under the old bug, playhead=2 would stay in range and land on day2's own index 2
        // (00:10), nowhere near 23:55.
        let day2_frames: Vec<(u32, u32)> = (0..288).map(|i| (i * 5 / 60, i * 5 % 60)).collect();
        t.set_frames(frames_at(day2, &day2_frames), ("KTLX".into(), day2));
        assert_eq!(
            t.current()
                .unwrap()
                .date_time()
                .unwrap()
                .format("%H:%M")
                .to_string(),
            "23:55",
            "should land near the same time of day that was showing before the jump"
        );
    }

    /// A day with nothing shown yet (fresh pane, no current frame) has no time of day to carry
    /// over — falls back to noon rather than panicking or picking an arbitrary hour.
    #[test]
    fn with_nothing_on_screen_yet_it_falls_back_to_noon() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap();
        let mut t = Timeline::default();
        seek_to_day(&mut t, "KTLX", day);
        t.set_frames(
            frames_at(day, &[(0, 0), (11, 55), (12, 0), (23, 55)]),
            ("KTLX".into(), day),
        );
        assert_eq!(
            t.current()
                .unwrap()
                .date_time()
                .unwrap()
                .format("%H:%M")
                .to_string(),
            "12:00"
        );
    }

    /// Jumping to today (or past it) must return to the live head via `follow_day`, not defer an
    /// archive seek that a same-day listing would then have to resolve against a moving target.
    #[test]
    fn jumping_to_today_goes_live_instead_of_seeking() {
        let today = chrono::Utc::now().date_naive();
        let mut t = Timeline::default();
        t.date = today - chrono::Duration::days(3);
        t.following = false;
        seek_to_day(&mut t, "KTLX", today);
        assert_eq!(t.date, today);
        assert!(t.following, "back on today should mean back on live");
    }
}

/// A small "still connected" ring beside the Live badge: a static arc showing how far the radar
/// has scanned into the current volume (the same fraction the bar below it draws linearly), plus
/// — unless motion is reduced — a short highlight segment that spins continuously while drawn, so
/// "the stream is open and nothing has stalled" is visible without reading the badge's own hover
/// text or watching the clock advance.
///
/// Static under [`crate::ui::motion::reduced`] rather than skipped outright: the progress arc
/// still answers "what tilt", the spin is only what answers "is it alive right now", and reduced
/// motion asked to drop the second question, not both of them.
fn live_progress_ring(ui: &mut egui::Ui, p: wxdata::live::ScanProgress, accent: egui::Color32) {
    const DIAM: f32 = 14.0;
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(DIAM, DIAM), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let center = rect.center();
    let radius = DIAM / 2.0 - 1.2;
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.4, egui::Color32::from_gray(90)),
    );
    let fraction = if p.total_elevations > 0 {
        (p.elevation_number as f32 / p.total_elevations as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // 12 o'clock start, sweeping clockwise like a clock face — not egui's own angle convention
    // (0 = 3 o'clock, increasing counter-clockwise).
    ring_arc(
        painter,
        center,
        radius,
        -std::f32::consts::FRAC_PI_2,
        fraction * std::f32::consts::TAU,
        egui::Stroke::new(2.0, accent),
    );
    if !crate::ui::motion::reduced() {
        let phase = ui.input(|i| i.time) as f32 * std::f32::consts::TAU * 0.5; // one turn ~2s
        ring_arc(
            painter,
            center,
            radius,
            phase,
            0.5,
            egui::Stroke::new(2.0, egui::Color32::WHITE.gamma_multiply(0.9)),
        );
        ui.ctx().request_repaint();
    }
}

/// A stroked arc from `start` (radians) sweeping `sweep` radians clockwise, sampled coarsely —
/// this draws a 12-14 px ring, not a chart, so a handful of segments already looks smooth.
fn ring_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    start: f32,
    sweep: f32,
    stroke: egui::Stroke,
) {
    let points = arc_points(center, radius, start, sweep);
    if points.len() >= 2 {
        painter.add(egui::Shape::line(points, stroke));
    }
}

/// The polyline for [`ring_arc`], split out so the geometry is testable without a `Painter`.
///
/// `start` is in radians using egui's screen convention (Y grows downward, so increasing angle
/// already turns clockwise as drawn) — `-FRAC_PI_2` is straight up, i.e. 12 o'clock, which is
/// where every caller here starts a sweep so the ring reads like a clock face rather than egui's
/// native 3-o'clock/counter-clockwise zero.
fn arc_points(center: egui::Pos2, radius: f32, start: f32, sweep: f32) -> Vec<egui::Pos2> {
    if sweep.abs() < 1e-3 {
        return Vec::new();
    }
    let steps = ((sweep.abs() / std::f32::consts::TAU) * 48.0)
        .ceil()
        .max(4.0) as usize;
    (0..=steps)
        .map(|i| {
            let t = start + sweep * (i as f32 / steps as f32);
            center + egui::vec2(t.cos(), t.sin()) * radius
        })
        .collect()
}

#[cfg(test)]
mod ring_tests {
    use super::*;

    /// The ring starts every sweep at 12 o'clock and reads clockwise as the fraction (elevation
    /// number over total elevations) grows — this pins down that "clockwise" actually means what
    /// it looks like on screen, not egui's own angle convention (0 = 3 o'clock, increasing
    /// counter-clockwise), which would draw the exact opposite of a clock face.
    #[test]
    fn a_quarter_turn_from_twelve_lands_at_three_oclock() {
        let center = egui::pos2(0.0, 0.0);
        let radius = 10.0;
        let points = arc_points(
            center,
            radius,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::FRAC_PI_2,
        );
        let first = *points.first().unwrap();
        let last = *points.last().unwrap();
        assert!((first.x).abs() < 1e-3 && (first.y - (-radius)).abs() < 1e-3); // 12 o'clock
        assert!((last.x - radius).abs() < 1e-3 && (last.y).abs() < 1e-3); // 3 o'clock
    }

    #[test]
    fn a_full_turn_ends_where_it_started() {
        let points = arc_points(egui::pos2(5.0, -3.0), 8.0, 0.3, std::f32::consts::TAU);
        let first = *points.first().unwrap();
        let last = *points.last().unwrap();
        assert!((first.x - last.x).abs() < 1e-3);
        assert!((first.y - last.y).abs() < 1e-3);
    }

    /// A stalled or brand-new pane (no progress arc yet) must draw nothing rather than a
    /// zero-length line egui would otherwise be asked to stroke.
    #[test]
    fn zero_sweep_draws_nothing() {
        assert!(arc_points(egui::pos2(0.0, 0.0), 10.0, 0.0, 0.0).is_empty());
    }

    /// The widget's actual paint output, not just its geometry helper: a mid-volume scan has to
    /// produce both the background track (a full circle) and the progress arc (a path) — the two
    /// pieces someone glancing at the pill actually sees, as opposed to `arc_points` returning
    /// sensible numbers nobody ever painted.
    #[test]
    fn live_progress_ring_paints_the_track_and_the_progress_arc() {
        let ctx = egui::Context::default();
        let progress = wxdata::live::ScanProgress {
            elevation_number: 3,
            total_elevations: 14,
            elevation_angle_deg: 0.9,
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: 120.0,
            azimuth_end_deg: 240.0,
            chunk_index: 2,
            chunks_in_sweep: 3,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 200.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            live_progress_ring(ui, progress, egui::Color32::from_rgb(255, 0, 0));
        });
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(s.shape, egui::Shape::Circle(_))),
            "expected the background track circle: {:?}",
            output.shapes
        );
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(s.shape, egui::Shape::Path(_))),
            "expected the progress arc: {:?}",
            output.shapes
        );
    }

    /// A volume with no dual-pol... no, a fresh connection with no reported total yet
    /// (`total_elevations == 0`) is a real, if brief, state — the fraction has to fall back to 0
    /// rather than divide by zero and paint a NaN-shaped arc.
    #[test]
    fn a_zero_total_elevations_does_not_panic_or_produce_nan() {
        let ctx = egui::Context::default();
        let progress = wxdata::live::ScanProgress {
            elevation_number: 0,
            total_elevations: 0,
            elevation_angle_deg: 0.0,
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: 1,
            chunks_in_sweep: 3,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 200.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            live_progress_ring(ui, progress, egui::Color32::from_rgb(255, 0, 0));
        });
    }
}

/// The time track: one tick per volume, hour labels, the forecast tail, the live loop window,
/// and a knob you can drag. Returns its rect for the tour's spotlight.
fn track(
    ui: &mut egui::Ui,
    t: &mut crate::timeline::Timeline,
    tz: Option<wxdata::tz::Tz>,
    accent: egui::Color32,
    live_window: usize,
    compact: bool,
) -> egui::Rect {
    let slots = t.slot_count();
    // On a phone the track is the one control the whole timeline turns on, so it gets a thumb-tall
    // hit area, a bar you can see and a knob you can grab; the 10 pt / 3 pt / 4 pt version is for a
    // pointer.
    let phone = compact && crate::platform::phone_layout();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            if phone {
                44.0
            } else if compact {
                10.0
            } else {
                34.0
            },
        ),
        egui::Sense::click_and_drag(),
    );
    let p = ui.painter_at(rect);
    let bar = if compact {
        let thickness = if phone { 8.0 } else { 3.0 };
        egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), thickness))
    } else {
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - 12.0),
            egui::pos2(rect.right(), rect.bottom() - 6.0),
        )
    };
    // Slot centres, so the first and last frames sit inside the track instead of half off it.
    let x_of = |i: usize| bar.left() + (i as f32 + 0.5) / slots as f32 * bar.width();
    p.rect_filled(bar, 3.0, egui::Color32::from_gray(60));

    let observed = t.frames.len();
    // The model tail is a different kind of time and says so, exactly as the old slider did.
    if slots > observed && observed > 0 {
        let x = x_of(observed);
        p.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, bar.top()),
                egui::pos2(bar.right(), bar.bottom()),
            ),
            3.0,
            egui::Color32::from_rgba_unmultiplied(120, 170, 240, 90),
        );
        if !compact {
            p.line_segment(
                [egui::pos2(x, rect.top() + 6.0), egui::pos2(x, bar.bottom())],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 170, 240)),
            );
        }
    }
    // The live loop window: the stretch ▶ actually cycles through when pinned to live. Without
    // it, pressing play on a day of frames looks like it jumped backwards for no reason.
    if t.following && observed > 0 {
        let from = observed.saturating_sub(live_window.max(1));
        p.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x_of(from), bar.top()),
                egui::pos2(x_of(observed.saturating_sub(1)), bar.bottom()),
            ),
            3.0,
            accent.gamma_multiply(0.35),
        );
    }
    // Played-so-far fill, then one tick per volume with an hour label wherever the hour turns
    // over — the axis a chaser reads to find "the 22Z scan" without scrubbing for it.
    p.rect_filled(
        egui::Rect::from_min_max(bar.left_top(), egui::pos2(x_of(t.playhead), bar.bottom())),
        3.0,
        accent.gamma_multiply(0.8),
    );
    if !compact {
        let mut last_hour = None;
        let mut last_label_x = f32::NEG_INFINITY;
        for (i, id) in t.frames.iter().enumerate() {
            let x = x_of(i);
            p.line_segment(
                [
                    egui::pos2(x, bar.top() - 3.0),
                    egui::pos2(x, bar.top() - 1.0),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
            );
            let Some(dt) = id.date_time() else { continue };
            let hour = hour_key(dt, tz);
            let turned = last_hour != Some(hour);
            last_hour = Some(hour);
            // Label the hour, not the volume: a five-minute clock on every tick is a smear. 96 px
            // of clearance, and both ends have to fit inside the track or the pill clips them.
            let half = 26.0;
            if !turned
                || x - last_label_x < 96.0
                || x - half < rect.left()
                || x + half > rect.right()
            {
                continue;
            }
            p.text(
                egui::pos2(x, rect.top() + 1.0),
                egui::Align2::CENTER_TOP,
                hour_label(dt, tz),
                egui::FontId::proportional(crate::ui::style::FONT_SM),
                egui::Color32::from_gray(170),
            );
            p.line_segment(
                [
                    egui::pos2(x, bar.top() - 6.0),
                    egui::pos2(x, bar.top() - 1.0),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_gray(150)),
            );
            last_label_x = x;
        }
    }
    let knob = egui::pos2(x_of(t.playhead), bar.center().y);
    let knob_radius = if phone {
        11.0
    } else if compact {
        4.0
    } else {
        7.0
    };
    p.circle_filled(knob, knob_radius, accent);
    p.circle_stroke(
        knob,
        knob_radius,
        egui::Stroke::new(1.0, egui::Color32::from_gray(230)),
    );

    // Click anywhere on the track, or drag the knob: both are the same "put the playhead here".
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
            let idx = ((frac * slots as f32) as usize).min(slots - 1);
            if idx != t.playhead {
                t.playhead = idx;
                t.playing = false;
                t.following = idx + 1 == observed;
                // One detent per frame: the track has no ticks under a thumb that covers it, so
                // the frames are felt instead.
                crate::platform::haptic(crate::platform::Haptic::Tick);
            }
        }
    }
    // The track is painted rather than built from widgets, so its accessibility node is empty
    // unless we fill it in. Where the playhead sits is the whole of what it says.
    let (at, of) = (t.playhead + 1, slots.max(1));
    resp.widget_info(|| {
        egui::WidgetInfo::slider(true, at as f64, format!("Timeline, frame {at} of {of}"))
    });
    rect
}

/// Which hour a frame falls in, in the radar's own zone — the axis the labels step along.
fn hour_key(dt: chrono::DateTime<Utc>, tz: Option<wxdata::tz::Tz>) -> (u32, u32) {
    use chrono::{Datelike, Timelike};
    match tz {
        Some(tz) => {
            let l = dt.with_timezone(&tz);
            (l.ordinal(), l.hour())
        }
        None => (dt.ordinal(), dt.hour()),
    }
}

/// Hour tick label: short enough to repeat across the track ("8 PM", or "20Z" in Zulu).
fn hour_label(dt: chrono::DateTime<Utc>, tz: Option<wxdata::tz::Tz>) -> String {
    match tz {
        Some(tz) => dt.with_timezone(&tz).format("%-I %p").to_string(),
        None => dt.format("%HZ").to_string(),
    }
}
