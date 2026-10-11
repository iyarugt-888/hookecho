//! Right-dock active-alerts panel: every NWS alert whose polygon overlaps the current view,
//! sorted by severity then soonest expiry. Clicking a row flies the camera to that alert.

use wxdata::overlay::{AlertInfo, GeoFeature};

/// Severity ordering for the panel: higher = more urgent, listed first.
pub fn severity_rank(event: &str) -> u8 {
    let e = event.to_ascii_lowercase();
    if e.contains("tornado") && e.contains("warning") {
        6
    } else if e.contains("flash flood") && e.contains("warning") {
        5
    } else if e.contains("severe thunderstorm") && e.contains("warning") {
        4
    // MeteoAlarm synthesizes its events as "{colour} {hazard} Warning" (see wxdata::meteoalarm),
    // so the awareness level is right there in the string — without this a Red (level 4) event
    // ranked exactly with a Yellow one, and Europe's worst day sorted like a drizzle advisory.
    } else if e.contains("red") && e.contains("warning") {
        5
    } else if e.contains("orange") && e.contains("warning") {
        4
    } else if e.contains("warning") {
        3
    } else if e.contains("watch") {
        2
    } else {
        1 // advisories, statements, etc.
    }
}

/// The alert bell's count badge, in the floating chrome and on the phone rail. The worst
/// escalation in view ([`wxdata::alerts::escalation`]) is shown three ways, never by colour alone
/// (ROADMAP_NEW Q3): the colour; a ring from CONSIDERABLE up; and "!" after the count for
/// DESTRUCTIVE or an observed tornado, "!!" for a Tornado Emergency or PDS, which also widens the
/// circle into a pill. Before this, both of the top tiers were the same red.
pub fn paint_bell_badge(painter: &egui::Painter, at: egui::Pos2, count: usize, esc: u8) {
    let (fill, ink, text, ring) = bell_badge(count, esc);
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(10.0), ink);
    let w = (galley.size().x + 8.0).max(16.0);
    let rect = egui::Rect::from_center_size(at, egui::vec2(w, 16.0));
    painter.rect_filled(rect, 8.0, fill);
    if ring {
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
            egui::StrokeKind::Outside,
        );
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
}

/// [`paint_bell_badge`]'s fill, text colour, text and whether it is ringed, for `count` alerts
/// whose worst escalation is `esc`.
fn bell_badge(count: usize, esc: u8) -> (egui::Color32, egui::Color32, String, bool) {
    let (fill, ink) = match esc {
        0 => (crate::ui::style::OMEGA_ORANGE, egui::Color32::BLACK),
        1 => (egui::Color32::from_rgb(230, 120, 60), egui::Color32::BLACK),
        // White on the red: black on it was hard to read.
        _ => (egui::Color32::from_rgb(200, 20, 20), egui::Color32::WHITE),
    };
    let mark = match esc {
        0 | 1 => "",
        2 => "!",
        _ => "!!",
    };
    (fill, ink, format!("{}{mark}", count.min(99)), esc >= 1)
}

/// The alert bell's accessible name and hover text: how many alerts are in view and the worst
/// of them, in words.
pub fn bell_name(count: usize, esc: u8) -> String {
    match (count, esc) {
        (0, _) => "Active alerts in view".to_owned(),
        (n, 0) => format!("Active alerts in view: {n}"),
        (n, 1) => {
            format!("Active alerts in view: {n}, the worst with a considerable damage threat")
        }
        (n, 2) => {
            format!("Active alerts in view: {n}, the worst destructive or with a tornado observed")
        }
        (n, _) => format!("Active alerts in view: {n}, including a Tornado Emergency or PDS"),
    }
}

#[cfg(test)]
mod severity_tests {
    use super::severity_rank;

    #[test]
    fn european_alerts_rank_by_colour() {
        // MeteoAlarm's synthesized events, in the shape wxdata::meteoalarm produces them.
        assert_eq!(severity_rank("Red wind Warning"), 5);
        assert_eq!(severity_rank("Orange rain Warning"), 4);
        assert_eq!(severity_rank("Yellow snow-ice Warning"), 3);
        // US events keep their own order, and tornado still outranks everything.
        assert_eq!(severity_rank("Tornado Warning"), 6);
        assert_eq!(severity_rank("Flash Flood Warning"), 5);
        assert_eq!(severity_rank("Severe Thunderstorm Warning"), 4);
        assert_eq!(severity_rank("Tornado Watch"), 2);
        assert_eq!(severity_rank("Special Weather Statement"), 1);
    }
}

/// One row's data: the alert, its stroke color, and its polygon-center (lon, lat) for fly-to.
pub struct Row<'a> {
    pub info: &'a AlertInfo,
    pub color: [u8; 4],
    pub center: (f64, f64),
    /// Escalation tier, computed once per row — `escalation()` uppercases the (multi-KB)
    /// bulletin text, too heavy to re-run inside a per-frame sort comparator.
    pub esc: u8,
}

/// Collect alert features whose bbox overlaps the view bounds `(min_lon, min_lat, max_lon, max_lat)`,
/// deduped by alert id, sorted by severity then soonest expiry.
pub fn rows_in_view(feats: &[GeoFeature], bounds: (f64, f64, f64, f64)) -> Vec<Row<'_>> {
    let (vx0, vy0, vx1, vy1) = bounds;
    // Borrowed ids: this runs every frame, and cloning each id was an allocation per alert.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut rows: Vec<Row> = Vec::new();
    for f in feats {
        let Some(a) = &f.alert else { continue };
        let Some((x0, y0, x1, y1)) = f.bbox() else {
            continue;
        };
        if x1 < vx0 || x0 > vx1 || y1 < vy0 || y0 > vy1 {
            continue; // bbox disjoint from the view
        }
        if !seen.insert(a.id.as_str()) {
            continue;
        }
        rows.push(Row {
            info: a,
            color: f.stroke,
            center: ((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            esc: wxdata::alerts::escalation(a),
        });
    }
    rows.sort_by(|a, b| {
        // Escalation leads: a PDS/destructive SVR outranks a plain TOR (matches broadcast practice).
        b.esc
            .cmp(&a.esc)
            .then_with(|| severity_rank(&b.info.event).cmp(&severity_rank(&a.info.event)))
            .then_with(|| expiry_key(a.info).cmp(&expiry_key(b.info)))
    });
    rows
}

/// Sort key for expiry: soonest first; missing expiry sinks to the bottom.
fn expiry_key(a: &AlertInfo) -> i64 {
    a.expires.map(|e| e.timestamp()).unwrap_or(i64::MAX)
}

/// Render the alerts list into whatever hosts it (the sidebar's Alerts tab on desktop, the bell
/// sheet on Android). Returns the clicked alert's `(id, center_lon, center_lat)`.
pub fn body(
    ui: &mut egui::Ui,
    feats: &[GeoFeature],
    bounds: (f64, f64, f64, f64),
    muted: &mut bool,
    at: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<(String, f64, f64)> {
    let rows = rows_in_view(feats, bounds);
    let mut clicked = None;
    {
        {
            ui.horizontal(|ui| {
                ui.weak(format!("{} in view", rows.len()));
                // Silence switch, where you look when something is making noise at you.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let glyph = if *muted {
                        egui_phosphor::regular::BELL_SLASH
                    } else {
                        egui_phosphor::regular::BELL
                    };
                    let hover = if *muted {
                        "Alert sounds are muted — click to unmute"
                    } else {
                        "Mute all alert sounds"
                    };
                    if ui
                        .selectable_label(*muted, egui::RichText::new(glyph).size(16.0))
                        .on_hover_text(hover)
                        .clicked()
                    {
                        *muted = !*muted;
                    }
                });
            });
            ui.separator();
            if rows.is_empty() {
                ui.weak("No alerts in view.");
                return None;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for row in &rows {
                    let a = row.info;
                    let resp = egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(egui::Stroke::new(1.0, color32(row.color)))
                        .corner_radius(egui::CornerRadius::same(5))
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(5.0, 15.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 1.0, color32(row.color));
                                ui.strong(&a.event);
                                // Emergency/PDS/destructive chip on escalated rows.
                                if row.esc >= 2 {
                                    let label = a.headline.to_ascii_uppercase();
                                    let chip = if label.contains("TORNADO EMERGENCY") {
                                        "TOR EMERGENCY"
                                    } else if a.damage_threat.as_deref().is_some_and(|d| {
                                        d.to_ascii_uppercase().contains("CATASTROPHIC")
                                    }) {
                                        "CATASTROPHIC"
                                    } else {
                                        "DESTRUCTIVE"
                                    };
                                    ui.label(
                                        egui::RichText::new(chip)
                                            .small()
                                            .strong()
                                            .color(egui::Color32::WHITE)
                                            .background_color(egui::Color32::from_rgb(200, 20, 20)),
                                    );
                                }
                            });
                            ui.add(
                                egui::Label::new(egui::RichText::new(&a.area).weak().small())
                                    .truncate(),
                            );
                            ui.label(
                                egui::RichText::new(crate::ui::warning_window::countdown(a, at))
                                    .small(),
                            );
                        })
                        .response;
                    if resp.interact(egui::Sense::click()).clicked() {
                        clicked = Some((a.id.clone(), row.center.0, row.center.1));
                    }
                    ui.add_space(4.0);
                }
            });
        }
    }
    clicked
}

fn color32(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worst alert in view is said in words, not left to the badge's colour.
    #[test]
    fn the_bell_says_how_many_and_how_bad() {
        assert_eq!(bell_name(0, 3), "Active alerts in view");
        assert_eq!(bell_name(4, 0), "Active alerts in view: 4");
        assert!(bell_name(2, 1).contains("considerable"));
        assert!(bell_name(2, 2).contains("destructive"));
        assert!(bell_name(1, 3).contains("Tornado Emergency"));
    }

    /// Each tier differs by more than its colour: a ring from tier 1, a mark from tier 2, and the
    /// two top tiers (the same red) by their marks.
    #[test]
    fn the_bell_badge_marks_each_tier_without_colour() {
        let tiers: Vec<_> = (0..=3).map(|esc| bell_badge(3, esc)).collect();
        let marks: Vec<(&str, bool)> = tiers.iter().map(|t| (t.2.as_str(), t.3)).collect();
        assert_eq!(
            marks,
            [("3", false), ("3", true), ("3!", true), ("3!!", true)]
        );
        assert_eq!(
            tiers[2].0, tiers[3].0,
            "the top tiers share a colour, so the marks must differ"
        );
        assert_eq!(bell_badge(250, 0).2, "99");
    }

    #[test]
    fn severity_orders_tornado_first() {
        assert!(severity_rank("Tornado Warning") > severity_rank("Severe Thunderstorm Warning"));
        assert!(severity_rank("Severe Thunderstorm Warning") > severity_rank("Flood Watch"));
        assert!(severity_rank("Flood Watch") > severity_rank("Special Weather Statement"));
    }
}
