//! The timeline under everything: transport, the Live/Archive state, the valid time, speed, the
//! archive calendar and a jump-to-time field on the top row; a track with one tick per frame and
//! the hours along it; the tilts of the volume and how much of the loop is already downloaded.

use super::*;
use crate::ui::a11y::Named as _;
use chrono::{DateTime, Utc};
use egui::{FontId, Sense, Stroke};
use egui_phosphor::regular as ph;

/// The panel's height: the three rows and their margins.
const TIMELINE_H: f32 = 122.0;
/// Labels on the track closer than this would run into each other; the later one is dropped.
const HOUR_LABEL_GAP: f32 = 52.0;

/// The frames at which the clock's hour changes, with that hour as the axis labels it: the first
/// frame always, then each frame whose hour differs from the frame before it. Frames with no
/// time (the forecast tail) are skipped.
pub(super) fn hour_marks(
    times: &[Option<DateTime<Utc>>],
    tz: Option<wxdata::tz::Tz>,
) -> Vec<(usize, String)> {
    let hour = |d: DateTime<Utc>| -> (String, String) {
        match tz {
            Some(tz) => {
                let l = d.with_timezone(&tz);
                (
                    l.format("%Y%m%d%H").to_string(),
                    l.format("%-I %p").to_string(),
                )
            }
            None => (
                d.format("%Y%m%d%H").to_string(),
                d.format("%H:00Z").to_string(),
            ),
        }
    };
    let mut out = Vec::new();
    let mut prev: Option<String> = None;
    for (i, d) in times.iter().enumerate() {
        let Some(d) = d else { continue };
        let (key, label) = hour(*d);
        if prev.as_deref() != Some(key.as_str()) {
            out.push((i, label));
            prev = Some(key);
        }
    }
    out
}

/// The centre of slot `i` of `n` across `[left, right]`.
pub(super) fn slot_x(i: usize, n: usize, left: f32, right: f32) -> f32 {
    if n <= 1 {
        return (left + right) / 2.0;
    }
    left + (right - left) * i as f32 / (n - 1) as f32
}

/// The slot nearest `x` across `[left, right]`, for a click or a drag on the track.
pub(super) fn slot_at(x: f32, n: usize, left: f32, right: f32) -> usize {
    if n <= 1 || right <= left {
        return 0;
    }
    let f = ((x - left) / (right - left)).clamp(0.0, 1.0);
    ((f * (n - 1) as f32).round() as usize).min(n - 1)
}

impl HookEchoApp {
    pub(super) fn dock_timeline(&mut self, root: &mut egui::Ui) {
        if !self.dock.timeline_open {
            return;
        }
        let t = self.ws_tokens();
        let tz = self.active_tz();
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "no site".to_string());
        // Which frames are already downloaded and decoded: the buffer the loop plays from.
        let cached: Vec<bool> = self.views[self.active]
            .timeline
            .frames
            .iter()
            .map(|id| self.scan_cache.contains(&id.name().to_string()))
            .collect();
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(view, _, _, _)| *view == self.active);
        let progress = self.views[self.active].live_progress;
        let indicator = self.settings.live_scan_indicator;
        let status = live_status(progress, streaming, indicator);
        let (elevations, tilt) = {
            let v = &self.views[self.active];
            (
                v.volume
                    .as_ref()
                    .map(|x| x.elevations.clone())
                    .unwrap_or_default(),
                v.tilt,
            )
        };
        let sweeping = sweeping_tilt(progress, &elevations, streaming, indicator);
        let mut go_head = false;
        let mut pick_tilt = None;
        let mut seek = None;
        egui::Panel::bottom("dock_timeline")
            .exact_size(TIMELINE_H)
            .resizable(false)
            .frame(
                ws::panel_frame(&t).inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 6,
                    bottom: 6,
                }),
            )
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                let tl = &mut self.views[self.active].timeline;
                let slots = tl.slot_count();
                let observed = tl.frames.len();
                // Row 1: transport, state, time, speed | archive, jump.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let b = |ui: &mut egui::Ui, glyph: &str, name: &str| {
                        ws::icon_button(ui, &t, glyph, "", false).named(name).clicked()
                    };
                    if b(ui, ph::SKIP_BACK, "Jump to start") {
                        tl.go_begin();
                    }
                    if b(ui, ph::REWIND, "Previous frame") {
                        tl.step(-1);
                    }
                    let playing = tl.playing;
                    if ws::icon_button(ui, &t, if playing { ph::PAUSE } else { ph::PLAY }, "", playing)
                        .named_toggle(if playing { "Pause" } else { "Play" }, playing)
                        .clicked()
                    {
                        tl.toggle_play();
                    }
                    if b(ui, ph::FAST_FORWARD, "Next frame") {
                        tl.step(1);
                    }
                    if b(ui, ph::SKIP_FORWARD, "Jump to newest") {
                        go_head = true;
                    }
                    ui.add_space(8.0);
                    let live = tl.following && tl.forecast_hour().is_none();
                    let pill = if live {
                        ws::badge(ui, &t, "Live", t.live)
                    } else {
                        ws::badge(ui, &t, "Archive", t.warn)
                    };
                    if pill
                        .interact(Sense::click())
                        .named(if live { "Following the newest scan" } else { "Go live" })
                        .clicked()
                    {
                        go_head = true;
                    }
                    ui.add_space(10.0);
                    let label = match tl.forecast_hour() {
                        Some(h) => format!("Forecast +{h} h"),
                        None => tl
                            .current()
                            .and_then(|id| id.date_time())
                            .map(|d| crate::timefmt::fmt_date_clock(d, tz))
                            .unwrap_or_else(|| "\u{2014}".to_string()),
                    };
                    ui.label(ws::mono(label, 14.0, egui::Color32::WHITE).strong());
                    ui.add_space(12.0);
                    ws::caption(ui, &t, "Speed");
                    egui::ComboBox::from_id_salt("dock_speed")
                        .width(64.0)
                        .selected_text(format!("{:.0} fps", tl.speed))
                        .show_ui(ui, |ui| {
                            for s in [2.0f32, 4.0, 6.0, 8.0, 12.0, 16.0] {
                                ui.selectable_value(&mut tl.speed, s, format!("{s:.0} fps"));
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let jump = ui.add(
                            egui::TextEdit::singleline(&mut self.dock.jump)
                                .hint_text("HH:MM UTC")
                                .desired_width(110.0),
                        )
                        .on_hover_text(
                            "A UTC time on the shown day (20:12), or a date and time                              (2013-05-20 20:12); Enter seeks there",
                        );
                        if jump.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(at) =
                                crate::ui::layers_panel::parse_utc_time(self.dock.jump.trim(), tl.date)
                            {
                                seek = Some(at.timestamp());
                                self.dock.jump.clear();
                            }
                        }
                        ws::caption(ui, &t, "Jump to");
                        // The archive lives behind the date: same day-seek path as the desktop
                        // scrubber's menu.
                        let cal = ws::icon_button(
                            ui,
                            &t,
                            ph::CALENDAR_BLANK,
                            &format!("{}", tl.date.format("%b %-d, %Y")),
                            false,
                        )
                        .named("Archive: pick a day");
                        egui::Popup::menu(&cal)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(240.0);
                                ui.horizontal(|ui| {
                                    if ui.button(ph::CARET_LEFT).named("Previous day").clicked() {
                                        if let Some(d) = tl.date.pred_opt() {
                                            crate::app::chrome::scrubber::seek_to_day(tl, &site, d);
                                        }
                                    }
                                    if let Some(d) = archive_day_input(ui, tl.date) {
                                        crate::app::chrome::scrubber::seek_to_day(tl, &site, d);
                                    }
                                    let is_today = tl.date >= Utc::now().date_naive();
                                    if ui
                                        .add_enabled(!is_today, egui::Button::new(ph::CARET_RIGHT))
                                        .named("Next day")
                                        .clicked()
                                    {
                                        if let Some(d) = tl.date.succ_opt() {
                                            crate::app::chrome::scrubber::seek_to_day(tl, &site, d);
                                        }
                                    }
                                });
                                if let Some(d) = archive_day_calendar(ui, tl.date) {
                                    crate::app::chrome::scrubber::seek_to_day(tl, &site, d);
                                }
                            });
                    });
                });
                ui.add_space(4.0);
                // Row 2: the track. One tick per slot, brighter where the frame is downloaded,
                // the playhead in the accent, and the hours underneath.
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 38.0),
                    Sense::click_and_drag(),
                );
                let rail_y = rect.top() + 14.0;
                let (left, right) = (rect.left() + 6.0, rect.right() - 6.0);
                let p = ui.painter();
                p.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(left, rail_y - 9.0),
                        egui::pos2(right, rail_y + 9.0),
                    ),
                    4.0,
                    t.field,
                );
                if slots > 0 {
                    for i in 0..slots {
                        let x = slot_x(i, slots, left, right);
                        let forecast = i >= observed;
                        let have = cached.get(i).copied().unwrap_or(false);
                        let color = if forecast {
                            category_color("Models").gamma_multiply(0.7)
                        } else if have {
                            t.text_dim
                        } else {
                            t.text_faint.gamma_multiply(0.6)
                        };
                        let h = if have || forecast { 6.0 } else { 4.0 };
                        if forecast {
                            // Split at the rail: a forecast hour differs from an observed frame
                            // in shape, not only in the models' tint (roadmap Q3).
                            for (a, b) in [(-h, -2.0), (2.0, h)] {
                                p.line_segment(
                                    [egui::pos2(x, rail_y + a), egui::pos2(x, rail_y + b)],
                                    Stroke::new(1.5, color),
                                );
                            }
                        } else {
                            p.line_segment(
                                [egui::pos2(x, rail_y - h), egui::pos2(x, rail_y + h)],
                                Stroke::new(1.5, color),
                            );
                        }
                    }
                    let x = slot_x(tl.playhead.min(slots - 1), slots, left, right);
                    p.line_segment(
                        [egui::pos2(x, rail_y - 11.0), egui::pos2(x, rail_y + 11.0)],
                        Stroke::new(2.0, t.accent),
                    );
                    p.circle_filled(egui::pos2(x, rail_y - 11.0), 4.0, t.accent);
                    let times: Vec<Option<DateTime<Utc>>> =
                        tl.frames.iter().map(|id| id.date_time()).collect();
                    let mut last_x = f32::NEG_INFINITY;
                    for (i, label) in hour_marks(&times, tz) {
                        let x = slot_x(i, slots, left, right);
                        if x - last_x < HOUR_LABEL_GAP {
                            continue;
                        }
                        last_x = x;
                        p.text(
                            egui::pos2(x, rect.bottom() - 1.0),
                            egui::Align2::CENTER_BOTTOM,
                            label,
                            FontId::proportional(10.5),
                            t.text_faint,
                        );
                    }
                    if resp.clicked() || resp.dragged() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let idx = slot_at(pos.x, slots, left, right);
                            if idx != tl.playhead {
                                tl.playhead = idx;
                                tl.playing = false;
                                tl.following = idx + 1 == observed;
                            }
                        }
                    }
                } else {
                    p.text(
                        egui::pos2(left + 8.0, rail_y),
                        egui::Align2::LEFT_CENTER,
                        if tl.listing { "Listing scans\u{2026}" } else { "No scans for this day" },
                        FontId::proportional(11.5),
                        t.text_dim,
                    );
                }
                // Row 3: frame counter, the tilts, what the radar is doing | the buffer. The row
                // is split into two fixed rects and the buffer side is built first, so its widget
                // ids do not shift when the number of tilts before it changes.
                let (row, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), Sense::hover());
                let split = row.right() - 200.0;
                let mut right = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("dock_buffer")
                        .max_rect(egui::Rect::from_min_max(egui::pos2(split, row.top()), row.max))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                let have = cached.iter().filter(|c| **c).count();
                right.label(ws::mono(format!("{have}/{observed}"), 11.0, t.text_dim));
                let (r, _) = right.allocate_exact_size(egui::vec2(96.0, 6.0), Sense::hover());
                right.painter().rect_filled(r, 3.0, t.field);
                if observed > 0 {
                    let mut fill = r;
                    fill.set_width(r.width() * have as f32 / observed as f32);
                    right
                        .painter()
                        .rect_filled(fill, 3.0, t.live.gamma_multiply(0.8));
                }
                ws::caption(&mut right, &t, "Buffer");
                let mut left = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("dock_tilts")
                        .max_rect(egui::Rect::from_min_max(row.min, egui::pos2(split, row.bottom())))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let ui = &mut left;
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(ws::mono(
                    format!("Frame {} / {}", (tl.playhead + 1).min(slots), slots),
                    11.0,
                    t.text_dim,
                ));
                ws::divider(ui, &t, 18.0);
                ws::caption(ui, &t, "Tilts");
                for (i, a) in elevations.iter().enumerate() {
                    let (r, dot) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), Sense::click());
                    let on = i == tilt;
                    let live = sweeping == Some(i);
                    let c = r.center();
                    if on {
                        ui.painter().circle_filled(c, 4.5, t.accent);
                    } else {
                        ui.painter().circle_filled(
                            c,
                            3.0,
                            if dot.hovered() { t.text } else { t.text_faint },
                        );
                    }
                    if live {
                        // A faint full ring, and over it an arc as far round as the sweep has
                        // got (chunk of chunks) — the ribbon's fill strip, in the dot's shape.
                        ui.painter()
                            .circle_stroke(c, 6.5, Stroke::new(1.0, t.live.gamma_multiply(0.35)));
                        if let Some(p) = progress {
                            let f = if p.chunks_in_sweep > 0 {
                                (p.chunk_index as f32 / p.chunks_in_sweep as f32).clamp(0.12, 1.0)
                            } else {
                                0.12
                            };
                            let n = (24.0 * f).ceil() as usize;
                            let arc: Vec<egui::Pos2> = (0..=n)
                                .map(|k| {
                                    let a = -std::f32::consts::FRAC_PI_2
                                        + std::f32::consts::TAU * f * k as f32 / n as f32;
                                    c + 6.5 * egui::vec2(a.cos(), a.sin())
                                })
                                .collect();
                            ui.painter()
                                .add(egui::Shape::line(arc, Stroke::new(1.8, t.live)));
                        }
                    }
                    let name = if live {
                        format!("Tilt {a:.1}\u{b0} (sweeping now)")
                    } else {
                        format!("Tilt {a:.1}\u{b0}")
                    };
                    if dot.named_toggle(&name, on).clicked() {
                        pick_tilt = Some(i);
                    }
                }
                if let Some(a) = elevations.get(tilt) {
                    ui.label(ws::mono(format!("{a:.1}\u{b0}"), 11.0, t.text));
                }
                if !status.is_empty() {
                    ws::divider(ui, &t, 18.0);
                    ui.label(ws::mono(&status, 11.0, t.live));
                }
            });
        if go_head {
            self.views[self.active].timeline.go_head();
        }
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(ts) = seek {
            let ctx = root.ctx().clone();
            self.apply_palette(crate::app::PaletteAction::SeekTime(ts), &ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2013, 5, 20, h, m, 0).unwrap())
    }

    #[test]
    fn the_axis_marks_each_new_hour_once() {
        let times = [
            at(19, 50),
            at(19, 55),
            at(20, 0),
            at(20, 4),
            None,
            at(21, 2),
        ];
        assert_eq!(
            hour_marks(&times, None),
            [
                (0, "19:00Z".to_string()),
                (2, "20:00Z".into()),
                (5, "21:00Z".into())
            ]
        );
        let chicago = wxdata::tz::site_tz("KTLX");
        let marks = hour_marks(&times, chicago);
        assert_eq!(marks[1], (2, "3 PM".to_string()));
    }

    #[test]
    fn a_slot_and_its_position_round_trip() {
        for n in [1usize, 2, 7, 40] {
            for i in 0..n {
                let x = slot_x(i, n, 10.0, 510.0);
                assert_eq!(slot_at(x, n, 10.0, 510.0), if n == 1 { 0 } else { i });
            }
        }
        // Past either end clamps to the end slots.
        assert_eq!(slot_at(-100.0, 5, 0.0, 100.0), 0);
        assert_eq!(slot_at(900.0, 5, 0.0, 100.0), 4);
    }
}
