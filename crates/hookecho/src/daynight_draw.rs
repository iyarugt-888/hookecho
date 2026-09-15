//! Day/night shading and the lat/lon graticule — a plain reference layer, not a data overlay: no
//! fetch, no feature list, just the sun's current position (see [`crate::astro::subsolar_point`])
//! turned into paint. Same `to_screen`-closure convention as [`crate::tropical_draw`] and
//! [`crate::outage_draw`], so it drops into `render_pane` the same way they do.
//!
//! The night side is drawn as a strip per longitude sample rather than one polygon that sweeps
//! the whole globe: a strip only ever spans a couple of degrees of longitude, so it is never at
//! risk of the antimeridian-wrap smear a single all-the-way-around polygon would need explicit
//! handling for. The terminator line and the graticule are open polylines/segments for the same
//! reason — nothing here ever closes a loop across the whole width of the map.

use crate::astro::{solar_zenith_cos, subsolar_point, terminator_lat_deg};
use chrono::{DateTime, Utc};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

/// Meridians drawn, every N degrees of longitude.
const MERIDIAN_STEP: f64 = 20.0;
/// Parallels drawn, every N degrees of latitude. The same step as the meridians (rather than a
/// conventional 15°) so `-80..=80` divides evenly and the grid comes out symmetric top to bottom.
const PARALLEL_STEP: f64 = 20.0;
/// Highest/lowest latitude any line reaches — web Mercator's own well-known cutoff (matching
/// [`crate::render::mercator::MAX_LAT`], not imported directly: this module only needs "a sane
/// pole cutoff", not the exact web-mercator limit).
const POLE_CUTOFF: f64 = 85.0;
/// Longitude samples the night shading and the terminator line are drawn at. Coarse is fine —
/// the terminator curves gently, and every sample is one drawn quad.
const SAMPLES: usize = 180;

// Plain functions rather than `const` colors: `Color32`'s alpha constructors are not known to be
// `const fn` in the egui version this crate pins, and a call site inside an ordinary function
// never has to care either way.
fn graticule_color() -> Color32 {
    Color32::from_white_alpha(50)
}
fn graticule_label_color() -> Color32 {
    Color32::from_white_alpha(150)
}
// Straight (unmultiplied) alpha for both: these are chosen as "this hue, at this opacity", not
// pre-scaled — unlike `tropical_draw`'s box background, which picks premultiplied values directly.
fn night_fill_color() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 4, 18, 95)
}
fn terminator_color() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 232, 170, 205)
}

/// Draw the night-side shading, the terminator line, and the lat/lon graticule. `to_screen`
/// projects `(lon, lat)`; `clip` is the pane rect; `now` is always the real clock — the sun's
/// position for an archived storm would need threading a non-live instant in here, which nothing
/// upstream currently tracks, so this always draws the sun where it is right now.
pub fn draw(
    painter: &Painter,
    clip: Rect,
    to_screen: impl Fn(f64, f64) -> Pos2,
    now: DateTime<Utc>,
) {
    let painter = painter.with_clip_rect(clip);
    let subsolar = subsolar_point(now);

    draw_night_shading(&painter, &to_screen, subsolar);
    draw_terminator_line(&painter, &to_screen, subsolar);
    draw_graticule(&painter, clip, &to_screen);
}

/// One translucent quad per longitude sample, from the terminator up to whichever pole is
/// currently dark at that meridian. Which pole that is comes from probing the actual zenith-angle
/// formula a fraction of a degree off the terminator, rather than reasoning about it in the
/// abstract — the formula already knows, and it is the one thing that cannot be gotten backwards.
fn draw_night_shading(
    painter: &Painter,
    to_screen: &impl Fn(f64, f64) -> Pos2,
    subsolar: (f64, f64),
) {
    let step = 360.0 / SAMPLES as f64;
    let mut lon = -180.0;
    for _ in 0..SAMPLES {
        let lon_hi = lon + step;
        let mid = lon + step * 0.5;
        if let Some(lat_term) = terminator_lat_deg(mid, subsolar) {
            let lat_term = lat_term.clamp(-POLE_CUTOFF, POLE_CUTOFF);
            let probe = (lat_term + 0.5).clamp(-89.9, 89.9);
            let night_is_north = solar_zenith_cos(probe, mid, subsolar) < 0.0;
            let (lat_a, lat_b) = if night_is_north {
                (lat_term, POLE_CUTOFF)
            } else {
                (-POLE_CUTOFF, lat_term)
            };
            let quad = vec![
                to_screen(lon, lat_a),
                to_screen(lon_hi, lat_a),
                to_screen(lon_hi, lat_b),
                to_screen(lon, lat_b),
            ];
            painter.add(Shape::convex_polygon(
                quad,
                night_fill_color(),
                Stroke::NONE,
            ));
        }
        // At the rare instant the sun sits exactly over the equator, `terminator_lat_deg` is
        // `None` for every longitude (see its own doc comment) — this sample's strip is simply
        // skipped rather than drawn wrong, and the very next sample a moment later draws fine.
        lon = lon_hi;
    }
}

/// The terminator itself: an open polyline from lon -180 to +180. Open, not a closed ring, so
/// there is no "last point back to first" edge to get wrong across the map's full width.
fn draw_terminator_line(
    painter: &Painter,
    to_screen: &impl Fn(f64, f64) -> Pos2,
    subsolar: (f64, f64),
) {
    let step = 360.0 / SAMPLES as f64;
    let mut pts = Vec::with_capacity(SAMPLES + 1);
    let mut lon = -180.0;
    for _ in 0..=SAMPLES {
        if let Some(lat) = terminator_lat_deg(lon, subsolar) {
            pts.push(to_screen(lon, lat.clamp(-POLE_CUTOFF, POLE_CUTOFF)));
        }
        lon += step;
    }
    if pts.len() >= 2 {
        painter.add(Shape::dashed_line(
            &pts,
            Stroke::new(1.5, terminator_color()),
            6.0,
            4.0,
        ));
    }
}

/// Meridians and parallels, each a single segment (both are straight lines end to end in web
/// Mercator), with one lat/lon label per line where it crosses the pane's top or left edge.
fn draw_graticule(painter: &Painter, clip: Rect, to_screen: &impl Fn(f64, f64) -> Pos2) {
    let font = FontId::proportional(10.0);

    let mut lon = -180.0;
    while lon <= 180.0 {
        let top = to_screen(lon, POLE_CUTOFF);
        let bottom = to_screen(lon, -POLE_CUTOFF);
        painter.line_segment([top, bottom], Stroke::new(1.0, graticule_color()));
        if top.x >= clip.left() && top.x <= clip.right() {
            painter.text(
                Pos2::new(top.x + 3.0, clip.top() + 3.0),
                Align2::LEFT_TOP,
                format_lon(lon),
                font.clone(),
                graticule_label_color(),
            );
        }
        lon += MERIDIAN_STEP;
    }

    let mut lat = -80.0;
    while lat <= 80.0 {
        let left = to_screen(-180.0, lat);
        let right = to_screen(180.0, lat);
        painter.line_segment([left, right], Stroke::new(1.0, graticule_color()));
        if left.y >= clip.top() && left.y <= clip.bottom() {
            painter.text(
                Pos2::new(clip.left() + 3.0, left.y + 2.0),
                Align2::LEFT_TOP,
                format_lat(lat),
                font.clone(),
                graticule_label_color(),
            );
        }
        lat += PARALLEL_STEP;
    }
}

fn format_lat(lat: f64) -> String {
    if lat == 0.0 {
        "0°".to_string()
    } else {
        format!("{:.0}°{}", lat.abs(), if lat > 0.0 { "N" } else { "S" })
    }
}

fn format_lon(lon: f64) -> String {
    if lon == 0.0 || lon.abs() == 180.0 {
        format!("{:.0}°", lon.abs())
    } else {
        format!("{:.0}°{}", lon.abs(), if lon > 0.0 { "E" } else { "W" })
    }
}
