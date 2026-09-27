//! The Alerts window: the warnings, watches and advisories in view, in the workstation's own rows
//! rather than the floating panel's cards (`ui::alert_panel`, which the panel's Alerts tab and the
//! phone sheet still draw; the order and the escalation tiers are that module's). Each row says
//! what the analyst reads a warning for — the event and its escalation, the hazard tags, the
//! office or area, when it was issued and until when — and its status is told against the time
//! on the map: the scrubbed frame's time in an archive view, the clock when live. An archived
//! tornado warning is "in effect" at the frame it covered, not "expired" against today. A row
//! flies the map to the alert and opens its bulletin.

use super::*;
use chrono::{DateTime, Utc};
use egui_phosphor::regular as ph;
use wxdata::overlay::AlertInfo;

/// The window's width, docked or floating.
pub(super) const ALERTS_W: f32 = 300.0;

/// Where an alert stands at the time on the map.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum AlertStatus {
    /// In effect, with the minutes left (None: no expiry given).
    InEffect(Option<i64>),
    Expired,
    NotYet,
}

impl AlertStatus {
    pub(super) fn at(
        issued: Option<DateTime<Utc>>,
        expires: Option<DateTime<Utc>>,
        at: DateTime<Utc>,
    ) -> AlertStatus {
        if issued.is_some_and(|i| i > at) {
            return AlertStatus::NotYet;
        }
        match expires {
            Some(e) if e <= at => AlertStatus::Expired,
            Some(e) => AlertStatus::InEffect(Some((e - at).num_minutes().max(0))),
            None => AlertStatus::InEffect(None),
        }
    }

    fn label(&self) -> String {
        match self {
            AlertStatus::InEffect(Some(m)) if *m >= 60 => format!("{}h {}m left", m / 60, m % 60),
            AlertStatus::InEffect(Some(m)) => format!("{m} min left"),
            AlertStatus::InEffect(None) => "in effect".into(),
            AlertStatus::Expired => "expired".into(),
            AlertStatus::NotYet => "not yet issued".into(),
        }
    }
}

/// When the alert was issued: the P-VTEC's start time (a continuation zeroes it, so then none),
/// else the archive's "Issued:" line.
pub(super) fn issued(a: &AlertInfo) -> Option<DateTime<Utc>> {
    let from_vtec = a.vtec.as_deref().and_then(|v| {
        let range = v.trim().trim_matches('/').split('.').nth(6)?;
        let start = range.split('-').next()?;
        chrono::NaiveDateTime::parse_from_str(start, "%y%m%dT%H%MZ")
            .ok()
            .map(|n| n.and_utc())
    });
    from_vtec.or_else(|| {
        let line = a
            .description
            .lines()
            .find_map(|l| l.strip_prefix("Issued: "))?;
        let line = line.trim();
        DateTime::parse_from_rfc3339(line)
            .map(|d| d.with_timezone(&Utc))
            .ok()
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(line, "%Y-%m-%dT%H:%MZ")
                    .ok()
                    .map(|n| n.and_utc())
            })
    })
}

/// "2:40 PM" in the site's zone, or "19:40Z".
fn clock(t: DateTime<Utc>, tz: Option<wxdata::tz::Tz>) -> String {
    match tz {
        Some(tz) => t.with_timezone(&tz).format("%-I:%M %p").to_string(),
        None => t.format("%H:%MZ").to_string(),
    }
}

/// The hazard tags a warning carries, short: "1.75 in hail · 60 MPH · tornado observed".
fn hazards(a: &AlertInfo) -> String {
    let mut out = Vec::new();
    if let Some(h) = a.max_hail_in.filter(|h| *h > 0.0) {
        out.push(format!("{h:.2} in hail"));
    }
    if let Some(w) = a.max_wind.as_deref().filter(|w| !w.is_empty()) {
        out.push(w.to_string());
    }
    if let Some(d) = a.tornado_detection.as_deref().filter(|d| !d.is_empty()) {
        out.push(format!("tornado {}", d.to_ascii_lowercase()));
    }
    out.join(" \u{b7} ")
}

/// The chip an escalated warning wears, as the panel names it.
fn escalation_chip(a: &AlertInfo) -> &'static str {
    if a.headline
        .to_ascii_uppercase()
        .contains("TORNADO EMERGENCY")
        || a.description.contains("TORNADO EMERGENCY")
    {
        "EMERGENCY"
    } else if a
        .damage_threat
        .as_deref()
        .is_some_and(|d| d.to_ascii_uppercase().contains("CATASTROPHIC"))
    {
        "CATASTROPHIC"
    } else if a.description.contains("PARTICULARLY DANGEROUS") {
        "PDS"
    } else {
        "DESTRUCTIVE"
    }
}

fn color32(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

impl HookEchoApp {
    pub(super) fn dock_alerts(&mut self, host: Host<'_>) {
        if !self.dock.alerts.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let (count, _) = self.alert_badge();
        let bounds = self.view_bounds();
        let feats = self.active_alert_features().to_vec();
        let tz = self.active_tz();
        // The time the list is read against: the scrubbed frame while archived warnings show.
        let archive = self.arch_warn_shown.is_some();
        let at = if archive {
            self.views[self.active]
                .volume
                .as_ref()
                .map_or_else(Utc::now, |v| v.time)
        } else {
            Utc::now()
        };
        let mut muted = self.settings.mute_alerts;
        let place = self.dock.alerts.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.alerts.collapsed;
        let list_h = (map_rect.height() - 60.0).clamp(160.0, 560.0);
        let title = if count == 0 {
            "Alerts".to_string()
        } else {
            format!("Alerts ({count})")
        };
        let mut rows = crate::ui::alert_panel::rows_in_view(&feats, bounds);
        let status: Vec<AlertStatus> = rows
            .iter()
            .map(|r| AlertStatus::at(issued(r.info), r.info.expires, at))
            .collect();
        // What is in effect leads; the panel's escalation/severity/expiry order holds within.
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by_key(|&i| !matches!(status[i], AlertStatus::InEffect(_)));
        let in_effect = status
            .iter()
            .filter(|s| matches!(s, AlertStatus::InEffect(_)))
            .count();
        let mut header = ws::HeaderAction::None;
        let mut hit = None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_alerts",
                place,
                width: ALERTS_W,
                // Beside where a floating Layers window starts, clear of the Inspector's corner.
                float_at: map_rect.left_top() + egui::vec2(12.0 + LEFT_WIDTH + 12.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::BELL,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let summary = if rows.is_empty() {
                                "None in view".to_string()
                            } else {
                                format!("{} in view \u{b7} {in_effect} in effect", rows.len())
                            };
                            ui.label(ws::text(summary, 11.5, t.text_dim));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let glyph = if muted { ph::BELL_SLASH } else { ph::BELL };
                                    let hover = if muted {
                                        "Alert sounds are muted: click to unmute"
                                    } else {
                                        "Mute all alert sounds"
                                    };
                                    if ws::icon_button(ui, &t, glyph, "", muted)
                                        .on_hover_text(hover)
                                        .clicked()
                                    {
                                        muted = !muted;
                                    }
                                },
                            );
                        });
                        if archive {
                            ui.label(ws::text(
                                format!(
                                    "Archive: status as of {}",
                                    crate::timefmt::fmt_clock(at, tz, false)
                                ),
                                10.5,
                                t.text_faint,
                            ));
                        }
                    });
                let scroll = egui::ScrollArea::vertical().auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(list_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 0,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            for &i in &order {
                                if let Some(h) = alert_row(ui, &t, &rows[i], &status[i], tz) {
                                    hit = Some(h);
                                }
                            }
                        });
                });
            },
        );
        rows.clear();
        self.dock.apply_header(DockWin::Alerts, header);
        self.settings.mute_alerts = muted;
        if let Some((id, lon, lat)) = hit {
            // Fly the active camera to the alert and open its bulletin, as the panel does.
            let cam = &mut self.views[self.active].camera;
            cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
            cam.zoom = cam.zoom.max(8.0);
            self.open_alert_popup(&id);
        }
    }
}

/// One alert: an edge in its map colour, the event (and an escalation chip), the hazard tags
/// and area, and its times with where it stands. Returns `(id, lon, lat)` when clicked.
fn alert_row(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    row: &crate::ui::alert_panel::Row<'_>,
    status: &AlertStatus,
    tz: Option<wxdata::tz::Tz>,
) -> Option<(String, f64, f64)> {
    let a = row.info;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 56.0), egui::Sense::click());
    let live = matches!(status, AlertStatus::InEffect(_));
    let edge = color32(row.color);
    let p = ui.painter();
    p.rect_filled(rect, 4.0, if resp.hovered() { t.field_hi } else { t.field });
    p.rect_filled(
        egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        if live {
            edge
        } else {
            edge.gamma_multiply(0.45)
        },
    );
    let x = rect.left() + 11.0;
    let right = rect.right() - 8.0;
    let ink = if live { t.text } else { t.text_dim };
    let title = p.layout_no_wrap(a.event.clone(), egui::FontId::proportional(12.5), ink);
    let title_w = title.size().x;
    p.galley(egui::pos2(x, rect.top() + 5.0), title, ink);
    if row.esc >= 2 {
        let chip = escalation_chip(a);
        let g = p.layout_no_wrap(
            chip.into(),
            egui::FontId::proportional(9.5),
            egui::Color32::WHITE,
        );
        let cr = egui::Rect::from_min_size(
            egui::pos2(
                (x + title_w + 6.0).min(right - g.size().x - 8.0),
                rect.top() + 6.0,
            ),
            egui::vec2(g.size().x + 8.0, 15.0),
        );
        p.rect_filled(cr, 3.0, t.danger);
        p.galley(
            cr.left_top() + egui::vec2(4.0, 1.5),
            g,
            egui::Color32::WHITE,
        );
    }
    let tags = hazards(a);
    // The archive names only the issuing office ("OUN"); say so rather than print a bare code.
    let bare_office =
        (3..=4).contains(&a.area.len()) && a.area.chars().all(|c| c.is_ascii_uppercase());
    let area = if bare_office {
        format!("NWS {} office", a.area)
    } else {
        a.area.clone()
    };
    let second = if tags.is_empty() {
        area
    } else {
        format!("{tags} \u{b7} {area}")
    };
    let mut job = egui::text::LayoutJob::single_section(
        second,
        egui::TextFormat::simple(egui::FontId::proportional(11.0), t.text_dim),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(right - x);
    let g = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x, rect.top() + 23.0), g, t.text_dim);
    let times = match (issued(a), a.expires) {
        (Some(i), Some(e)) => format!("{} \u{2013} {}", clock(i, tz), clock(e, tz)),
        (None, Some(e)) => format!("until {}", clock(e, tz)),
        (Some(i), None) => format!("from {}", clock(i, tz)),
        (None, None) => String::new(),
    };
    p.text(
        egui::pos2(x, rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        times,
        egui::FontId::monospace(10.5),
        t.text_faint,
    );
    let status_ink = match status {
        AlertStatus::InEffect(Some(m)) if *m <= 10 => t.warn,
        AlertStatus::InEffect(_) => t.live,
        _ => t.text_faint,
    };
    p.text(
        egui::pos2(right, rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        status.label(),
        egui::FontId::monospace(10.5),
        status_ink,
    );
    let resp = resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if a.headline.is_empty() || a.headline == a.event {
            "Fly to it and open the bulletin".to_string()
        } else {
            format!("{}\nClick to fly to it and open the bulletin", a.headline)
        });
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}, {}", a.event, status.label()),
        )
    });
    resp.clicked()
        .then(|| (a.id.clone(), row.center.0, row.center.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 20, h, m, 0).unwrap()
    }

    #[test]
    fn status_is_told_against_the_map_time() {
        let (i, e) = (Some(at(19, 40)), Some(at(20, 15)));
        assert_eq!(
            AlertStatus::at(i, e, at(20, 12)),
            AlertStatus::InEffect(Some(3))
        );
        assert_eq!(AlertStatus::at(i, e, at(20, 15)), AlertStatus::Expired);
        assert_eq!(AlertStatus::at(i, e, at(19, 0)), AlertStatus::NotYet);
        assert_eq!(
            AlertStatus::at(None, None, at(19, 0)),
            AlertStatus::InEffect(None)
        );
        assert_eq!(AlertStatus::InEffect(Some(75)).label(), "1h 15m left");
    }

    #[test]
    fn issue_time_comes_from_vtec_or_the_archive_line() {
        let mut a = AlertInfo {
            id: String::new(),
            event: "Tornado Warning".into(),
            headline: String::new(),
            area: String::new(),
            description: String::new(),
            instruction: String::new(),
            expires: None,
            max_hail_in: None,
            max_wind: None,
            tornado_detection: None,
            damage_threat: None,
            source: None,
            motion: None,
            vtec: Some("/O.NEW.KOUN.TO.W.0023.130520T1940Z-130520T2015Z/".into()),
        };
        assert_eq!(issued(&a), Some(at(19, 40)));
        a.vtec = None;
        a.description =
            "Tornado Warning\n\nWFO: OUN\nIssued: 2013-05-20T19:40:00Z\nExpires: x".into();
        assert_eq!(issued(&a), Some(at(19, 40)));
        a.description = "Issued: 2013-05-20T19:40Z".into();
        assert_eq!(issued(&a), Some(at(19, 40)));
    }
}
