//! Y'all mode in the app: the Y'all-O-Meter card and Y'all Tracks on the map, read at your GPS
//! fix or else the map's centre. The rules are [`crate::yall`]; this module gathers the inputs
//! (warnings, watches, storm cells the app already holds, plus the Day 1-3 categorical outlooks it
//! fetches itself while the mode is on) and draws the result.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;
use crate::yall::{self, Meter, Risk, YallTrack, YallWatch};
use wxdata::clock::Instant;
use wxdata::overlay::GeoFeature;

/// Categorical outlooks are refetched this often while the mode is on.
const OUTLOOK_REFRESH_S: u64 = 30 * 60;

#[derive(Default)]
pub(crate) struct YallState {
    /// Day 1..=3 SPC categorical outlooks.
    outlooks: [Vec<GeoFeature>; 3],
    fetched: Option<Instant>,
    rx: Option<std::sync::mpsc::Receiver<[Vec<GeoFeature>; 3]>>,
    /// The card shows every section, not only the meter.
    expanded: bool,
}

/// Everything the card and the map draw, read once a frame.
pub(crate) struct YallReading {
    pub spot: [f64; 2],
    pub from_gps: bool,
    pub meter: Meter,
    pub watches: Vec<YallWatch>,
    pub outlooks: [Option<Risk>; 3],
    pub tracks: Vec<YallTrack>,
}

impl HookEchoApp {
    /// While Y'all mode is on, keep the Day 1-3 categorical outlooks fresh.
    pub(crate) fn sync_yall(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.yall.rx {
            match rx.try_recv() {
                Ok(o) => {
                    self.yall.rx = None;
                    self.yall.outlooks = o;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.yall.rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        if !self.settings.yall_mode {
            return;
        }
        if self
            .yall
            .fetched
            .is_some_and(|t| t.elapsed().as_secs() < OUTLOOK_REFRESH_S)
        {
            return;
        }
        self.yall.fetched = Some(Instant::now());
        let (tx, rx) = std::sync::mpsc::channel();
        self.yall.rx = Some(rx);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let mut out: [Vec<GeoFeature>; 3] = Default::default();
            for (i, slot) in out.iter_mut().enumerate() {
                match wxdata::spc::fetch_outlook(&http, i as u8 + 1).await {
                    Ok(f) => *slot = f,
                    Err(e) => log::warn!("y'all mode: day {} outlook ({e})", i + 1),
                }
            }
            let _ = tx.send(out);
            ctx.request_repaint();
        });
    }

    /// The spot Y'all mode reads: your GPS fix, else the active pane's centre.
    fn yall_spot(&self) -> ([f64; 2], bool) {
        if let Some((lon, lat)) = self.chase_pos {
            return ([lon, lat], true);
        }
        let cam = &self.views[self.active].camera;
        let (lon, lat) = crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
        ([lon, lat], false)
    }

    pub(crate) fn yall_reading(&self) -> Option<YallReading> {
        if !self.settings.yall_mode || self.views.is_empty() {
            return None;
        }
        let (spot, from_gps) = self.yall_spot();
        let [lon, lat] = spot;
        let alerts = self.active_alert_features();
        let here: Vec<&GeoFeature> = alerts
            .iter()
            .filter(|f| f.alert.is_some() && f.contains(lon, lat))
            .fold(Vec::new(), |mut v: Vec<&GeoFeature>, f| {
                // One row per alert: a multi-part polygon arrives as several features.
                let id = f.alert.as_ref().map(|a| a.id.as_str());
                if !v
                    .iter()
                    .any(|g| g.alert.as_ref().map(|a| a.id.as_str()) == id)
                {
                    v.push(f);
                }
                v
            });
        let watches = yall::watches_at(&self.watch_features, alerts, lon, lat);
        let outlooks = [0, 1, 2].map(|d| yall::outlook_at(&self.yall.outlooks[d], lon, lat));
        let tracks = yall::tracks_toward(self.active_storm_cells(), spot);
        let meter = yall::meter(&here, &watches, outlooks[0], &tracks);
        Some(YallReading {
            spot,
            from_gps,
            meter,
            watches,
            outlooks,
            tracks,
        })
    }

    /// Y'all Tracks on the map: a line from each storm headed for the spot to where it passes
    /// closest, with the time, and a ring at the spot itself.
    pub(crate) fn draw_yall_tracks(
        &self,
        painter: &egui::Painter,
        screen: &dyn Fn(&[f64; 2]) -> egui::Pos2,
    ) {
        let Some(r) = self.yall_reading() else {
            return;
        };
        let [cr, cg, cb] = Meter::rgb(r.meter.level);
        let col = egui::Color32::from_rgb(cr, cg, cb);
        let at = screen(&r.spot);
        painter.circle_stroke(at, 9.0, egui::Stroke::new(2.5, egui::Color32::BLACK));
        painter.circle_stroke(at, 9.0, egui::Stroke::new(1.5, col));
        painter.circle_filled(at, 2.5, col);
        for t in &r.tracks {
            let a = screen(&t.from);
            let b = screen(&t.closest);
            let tc = if t.rotation {
                egui::Color32::from_rgb(230, 60, 50)
            } else if t.severe() {
                egui::Color32::from_rgb(245, 150, 50)
            } else {
                egui::Color32::from_rgb(240, 210, 70)
            };
            let dashes = egui::Shape::dashed_line(&[a, b], egui::Stroke::new(2.5, tc), 8.0, 5.0);
            painter.extend(dashes);
            painter.circle_filled(b, 4.0, tc);
            let label = if t.eta_min < 2.0 {
                format!("{} here now", t.id)
            } else {
                format!("{} ~{:.0} min", t.id, t.eta_min)
            };
            let font = egui::FontId::proportional(12.0);
            let pos = b + egui::vec2(7.0, -7.0);
            painter.text(
                pos + egui::vec2(1.0, 1.0),
                egui::Align2::LEFT_BOTTOM,
                &label,
                font.clone(),
                egui::Color32::BLACK,
            );
            painter.text(pos, egui::Align2::LEFT_BOTTOM, &label, font, tc);
        }
    }

    /// The Y'all-O-Meter card, bottom left of the map.
    pub(crate) fn yall_card(&mut self, ctx: &egui::Context) {
        let Some(r) = self.yall_reading() else {
            return;
        };
        let tz = self.active_tz();
        let dy = if self.chase_mode {
            crate::ui::style::LANE_BOTTOM_CHASE - 150.0
        } else {
            crate::ui::style::LANE_BOTTOM_CHASE
        };
        let mut expanded = self.yall.expanded;
        egui::Area::new(egui::Id::new("yall_card"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(14.0, dy))
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, 240).show(ui, |ui| {
                    ui.set_width(270.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Y'all-O-Meter").strong().size(14.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let arrow = if expanded {
                                format!("Less {}", egui_phosphor::regular::CARET_UP)
                            } else {
                                format!("More {}", egui_phosphor::regular::CARET_DOWN)
                            };
                            if ui.small_button(arrow).clicked() {
                                expanded = !expanded;
                            }
                        });
                    });
                    ui.weak(if r.from_gps {
                        "for your location"
                    } else {
                        "for the middle of the map"
                    });
                    gauge(ui, r.meter.level);
                    let [cr, cg, cb] = Meter::rgb(r.meter.level);
                    ui.label(
                        egui::RichText::new(r.meter.headline())
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(cr, cg, cb)),
                    );
                    let shown = if expanded { usize::MAX } else { 3 };
                    for reason in r.meter.reasons.iter().take(shown) {
                        ui.label(egui::RichText::new(format!("\u{2022} {reason}")).size(12.0));
                    }
                    if !expanded {
                        return;
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Y'all Watches").strong());
                    if r.watches.is_empty() {
                        ui.weak("No watches over y'all.");
                    }
                    for w in &r.watches {
                        let col = if w.tornado {
                            egui::Color32::from_rgb(255, 90, 90)
                        } else {
                            egui::Color32::from_rgb(240, 210, 70)
                        };
                        let until = w
                            .expires
                            .map(|t| {
                                let s = match tz {
                                    Some(z) => t.with_timezone(&z).format("%-I:%M %p %Z"),
                                    None => t.format("%H:%MZ"),
                                };
                                format!(" until {s}")
                            })
                            .unwrap_or_default();
                        ui.label(
                            egui::RichText::new(format!("{}{until}", w.title))
                                .color(col)
                                .strong(),
                        );
                        ui.label(egui::RichText::new(w.plain).size(12.0));
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Y'all Outlook").strong());
                    for (d, risk) in r.outlooks.iter().enumerate() {
                        let day = ["Today", "Tomorrow", "Day 3"][d];
                        match risk {
                            Some(risk) => {
                                let [rr, rg, rb] = risk.rgb();
                                ui.label(
                                    egui::RichText::new(format!("{day}: {}", risk.name()))
                                        .color(egui::Color32::from_rgb(rr, rg, rb)),
                                );
                                if d == 0 {
                                    ui.label(egui::RichText::new(risk.plain()).size(12.0));
                                }
                            }
                            None => {
                                ui.weak(format!("{day}: no severe risk"));
                            }
                        }
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Y'all Tracks").strong());
                    if r.tracks.is_empty() {
                        ui.weak(format!(
                            "No storms headed for y'all in the next {:.0} min.",
                            yall::TRACK_HORIZON_MIN
                        ));
                    }
                    for t in &r.tracks {
                        ui.label(egui::RichText::new(t.plain()).size(12.0));
                    }
                });
            });
        self.yall.expanded = expanded;
    }
}

/// A half-dial of six coloured segments with a needle at `level`.
fn gauge(ui: &mut egui::Ui, level: u8) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(270.0, 70.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let c = egui::pos2(rect.center().x, rect.bottom() - 6.0);
    let (r_out, r_in) = (60.0_f32, 42.0_f32);
    let seg = std::f32::consts::PI / 6.0;
    for i in 0..6u8 {
        let a0 = std::f32::consts::PI + i as f32 * seg;
        let pts: Vec<egui::Pos2> = (0..=8)
            .map(|k| a0 + seg * k as f32 / 8.0)
            .map(|a| c + egui::vec2(a.cos(), a.sin()) * r_out)
            .chain(
                (0..=8)
                    .rev()
                    .map(|k| a0 + seg * k as f32 / 8.0)
                    .map(|a| c + egui::vec2(a.cos(), a.sin()) * r_in),
            )
            .collect();
        let [r, g, b] = Meter::rgb(i);
        let alpha = if i == level { 255 } else { 110 };
        let mesh = convex_fan(&pts, egui::Color32::from_rgba_unmultiplied(r, g, b, alpha));
        p.add(egui::Shape::mesh(mesh));
    }
    let a = std::f32::consts::PI + (level as f32 + 0.5) * seg;
    let tip = c + egui::vec2(a.cos(), a.sin()) * (r_out + 2.0);
    p.line_segment([c, tip], egui::Stroke::new(3.0, egui::Color32::WHITE));
    p.circle_filled(c, 5.0, egui::Color32::WHITE);
}

/// A ring segment as a triangle strip: `pts` runs along the outer arc and back along the inner.
fn convex_fan(pts: &[egui::Pos2], col: egui::Color32) -> egui::Mesh {
    let mut m = egui::Mesh::default();
    for q in pts {
        m.colored_vertex(*q, col);
    }
    let n = pts.len() as u32 / 2;
    for k in 0..n - 1 {
        let (o0, o1) = (k, k + 1);
        let (i0, i1) = (2 * n - 1 - k, 2 * n - 2 - k);
        m.add_triangle(o0, o1, i0);
        m.add_triangle(o1, i1, i0);
    }
    m
}
