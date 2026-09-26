//! The status footer (roadmap Q2's "high-information status footer option"): one monospace line
//! under the timeline with what an analyst glances at constantly — where the pointer is, its range
//! and bearing from the radar, the beam's height there and the value; which pane, product and tilt
//! are active and how old the frame is; and how fast the app is drawing. Off by default (the map
//! keeps its pixels), saved with the arrangement like the timeline.

use super::*;
use egui::Sense;

const FOOTER_H: f32 = 22.0;

/// What the footer says, pure so it can be tested without a window.
#[derive(Debug, Default, PartialEq)]
pub(super) struct FooterText {
    pub pointer: String,
    pub pane: String,
    pub perf: String,
}

/// Build the footer's three parts. `reading` is the gate under the pointer as `(lon, lat,
/// value text, beam height text)`; `radar` the active site's position; `frame_age_s` the frame's
/// age; `frame_ms` the smoothed frame time.
#[allow(clippy::too_many_arguments)]
pub(super) fn footer_text(
    pointer: Option<(f64, f64)>,
    reading: Option<(String, String)>,
    radar: Option<(f64, f64)>,
    metric: bool,
    pane: (usize, usize),
    product: &str,
    tilt: Option<f32>,
    frame_age_s: Option<i64>,
    frame_ms: f32,
    zoom: f64,
) -> FooterText {
    let pointer = match pointer {
        Some(at) => {
            let mut parts: Vec<String> = Vec::new();
            for (k, v) in cursor_readout(at, radar, metric) {
                parts.push(match k {
                    "Lat" | "Lon" => v,
                    other => format!("{other} {v}"),
                });
            }
            if let Some((value, beam)) = reading {
                parts.push(format!("beam {beam}"));
                parts.push(value);
            }
            parts.join("  \u{b7}  ")
        }
        None => "pointer off the map".to_string(),
    };
    let mut p = vec![
        format!("Pane {}/{}", pane.0 + 1, pane.1),
        product.to_string(),
    ];
    if let Some(a) = tilt {
        p.push(format!("{a:.1}\u{b0}"));
    }
    if let Some(s) = frame_age_s {
        p.push(format!("frame {} old", humanize(s.max(0))));
    }
    FooterText {
        pointer,
        pane: p.join("  \u{b7}  "),
        perf: format!("{frame_ms:.1} ms  \u{b7}  z{zoom:.1}"),
    }
}

impl HookEchoApp {
    pub(super) fn dock_footer(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        // Smoothed even while hidden, so it reads true the moment it is shown.
        let dt = ctx.input(|i| i.unstable_dt) * 1000.0;
        self.dock.frame_ms = if self.dock.frame_ms > 0.0 {
            self.dock.frame_ms * 0.9 + dt * 0.1
        } else {
            dt
        };
        if !self.dock.footer_open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let single = self.views.len() == 1;
        let cam = self.views[self.active].camera;
        let pointer = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| map_rect.contains(*p) && single)
            .map(|p| {
                let w = cam.screen_to_world(
                    (p.x - map_rect.left(), p.y - map_rect.top()),
                    self.last_viewport,
                );
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            });
        let metric = self.metric_in(self.active);
        let reading = pointer
            .and_then(|(lon, lat)| self.dock_probe(lon, lat))
            .map(|p| {
                let moment = self.views[self.active].moment;
                let (f, unit) = display_units(moment, &self.settings);
                let value = match p.value {
                    Some(x) => format!("{:.1} {unit}", x * f),
                    None if p.folded => "range folded".into(),
                    None => "below threshold".into(),
                };
                (value, inspector::fmt_beam(p.beam_ft, metric))
            });
        let v = &self.views[self.active];
        let radar = v
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| (f64::from(s.longitude), f64::from(s.latitude)));
        let tilt = v
            .volume
            .as_ref()
            .and_then(|x| x.elevations.get(v.tilt).copied());
        let age = v
            .timeline
            .current()
            .and_then(|id| id.date_time())
            .map(|d| (chrono::Utc::now() - d).num_seconds());
        let text = footer_text(
            pointer,
            reading,
            radar,
            metric,
            (self.active, self.views.len()),
            crate::products::name(v.moment, v.srv),
            tilt,
            age,
            self.dock.frame_ms,
            v.camera.zoom,
        );
        egui::Panel::bottom("dock_footer")
            .exact_size(FOOTER_H)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .stroke(egui::Stroke::new(1.0, t.line_soft))
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(root, |ui| {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                let y = rect.center().y;
                let font = egui::FontId::monospace(11.0);
                let p = ui.painter_at(rect);
                // Right and middle are placed first so the pointer text on the left can be cut
                // short before it runs under them.
                let perf = p.layout_no_wrap(text.perf, font.clone(), t.text_faint);
                let perf_x = rect.right() - perf.size().x;
                let pane = p.layout_no_wrap(text.pane, font.clone(), t.text_dim);
                let pane_x = (perf_x - 24.0 - pane.size().x).max(rect.left() + rect.width() * 0.45);
                let mut job = egui::text::LayoutJob::simple_singleline(text.pointer, font, t.text);
                job.wrap = egui::text::TextWrapping::truncate_at_width(
                    (pane_x - 24.0 - rect.left()).max(40.0),
                );
                let pointer = ui.fonts_mut(|f| f.layout_job(job));
                p.galley(
                    egui::pos2(rect.left(), y - pointer.size().y / 2.0),
                    pointer,
                    t.text,
                );
                if pane_x + pane.size().x < perf_x {
                    p.galley(
                        egui::pos2(pane_x, y - pane.size().y / 2.0),
                        pane,
                        t.text_dim,
                    );
                }
                p.galley(
                    egui::pos2(perf_x, y - perf.size().y / 2.0),
                    perf,
                    t.text_faint,
                );
            });
        // The readout follows the pointer; keep it current while the pointer is still.
        if pointer.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_says_where_what_and_how_fast() {
        let t = footer_text(
            Some((-97.44, 35.33)),
            Some(("52.5 dBZ".into(), "4200 ft".into())),
            Some((-97.28, 35.33)),
            false,
            (0, 2),
            "Rain intensity (reflectivity)",
            Some(0.5),
            Some(125),
            16.64,
            8.0,
        );
        assert!(
            t.pointer.starts_with("35.33  \u{b7}  -97.44"),
            "{}",
            t.pointer
        );
        assert!(t.pointer.contains("Range") && t.pointer.contains("beam 4200 ft"));
        assert!(t.pointer.ends_with("52.5 dBZ"));
        assert!(t.pane.starts_with("Pane 1/2") && t.pane.contains("0.5\u{b0}"));
        assert!(t.pane.contains("old"));
        assert_eq!(t.perf, "16.6 ms  \u{b7}  z8.0");
        let off = footer_text(
            None,
            None,
            None,
            true,
            (0, 1),
            "Velocity",
            None,
            None,
            8.0,
            5.0,
        );
        assert_eq!(off.pointer, "pointer off the map");
        assert_eq!(off.pane, "Pane 1/1  \u{b7}  Velocity");
    }
}
