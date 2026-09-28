//! The route window (ROADMAP_NEW L1-L3): the routing server, the waypoints clicked with the Route
//! tool, the routes a server returns, progress along the chosen one from the chase position, and
//! where it runs into active warning polygons.
//!
//! It reports what the route meets. It never calls a route safe: a warning can be issued over any
//! road at any moment, and the window says so under every exposure list.

use wxdata::route::{Engine, Route};

#[derive(Default)]
pub struct RouteWindow {
    pub open: bool,
    /// `[lon, lat]`, start first.
    pub waypoints: Vec<[f64; 2]>,
    pub routes: Vec<Route>,
    pub selected: usize,
    pub busy: bool,
    pub error: Option<String>,
    /// Bumped whenever `routes` changes, so derived results know to recompute.
    pub generation: u64,
}

/// What the window asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAction {
    None,
    /// The waypoints or server changed: fetch routes again.
    Fetch,
    /// Put the chase/GPS position in as the start.
    StartHere,
}

/// Everything the app works out for the window each frame.
pub struct RouteReadout<'a> {
    pub metric: bool,
    pub have_position: bool,
    /// Distance left (km) and time left (s) on the chosen route from the chase position, when
    /// the position is on or near it.
    pub remaining: Option<(f64, f64)>,
    /// `(what, km ahead, seconds ahead)` for each polygon the chosen route enters, nearest first.
    pub exposure: &'a [(String, f64, f64)],
}

fn fmt_duration(s: f64) -> String {
    let m = (s / 60.0).round() as i64;
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

impl RouteWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        engine: &mut Engine,
        url: &mut String,
        readout: &RouteReadout,
        drawer: &mut crate::ui::drawer::Drawer,
    ) -> RouteAction {
        if !self.open {
            return RouteAction::None;
        }
        let mut open = self.open;
        let mut action = RouteAction::None;
        let Some(window) = drawer.page_sized(
            ctx,
            "Route",
            &mut open,
            false,
            380.0,
            egui::Window::new("Route"),
        ) else {
            self.open = open;
            return action;
        };
        window.show(ctx, |ui| {
            // The server: the user's own, or a public demo they choose knowingly.
            ui.horizontal(|ui| {
                ui.label("Server");
                egui::ComboBox::from_id_salt("route_engine")
                    .selected_text(engine.label())
                    .show_ui(ui, |ui| {
                        for e in [Engine::Osrm, Engine::Valhalla] {
                            if ui.selectable_value(engine, e, e.label()).changed() {
                                action = RouteAction::Fetch;
                            }
                        }
                    });
                if ui
                    .add(egui::TextEdit::singleline(url).hint_text("https://your-osrm-server"))
                    .lost_focus()
                {
                    action = RouteAction::Fetch;
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .small_button("Use the public demo server")
                    .on_hover_text(
                        "The project's own demo server: light use only (about one request a \
                         second), no guarantees. For regular chasing, run your own.",
                    )
                    .clicked()
                {
                    *url = engine.demo_url().to_string();
                    action = RouteAction::Fetch;
                }
                ui.weak("Light use only; your own OSRM or Valhalla server is better.");
            });
            ui.separator();
            // Waypoints.
            if self.waypoints.is_empty() {
                ui.weak(
                    "Click the map with the Route tool: start, any stops, then the destination.",
                );
            }
            let mut remove = None;
            for (i, p) in self.waypoints.iter().enumerate() {
                ui.horizontal(|ui| {
                    let tag = if i == 0 {
                        "Start".to_string()
                    } else if i + 1 == self.waypoints.len() {
                        "End".to_string()
                    } else {
                        format!("Stop {i}")
                    };
                    ui.strong(tag);
                    ui.weak(format!("{:.3}, {:.3}", p[1], p[0]));
                    if ui
                        .small_button("✕")
                        .on_hover_text("Remove this point")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.waypoints.remove(i);
                action = RouteAction::Fetch;
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        readout.have_position,
                        egui::Button::new("Start from my position"),
                    )
                    .on_disabled_hover_text("Set your position with the Chase tool or GPS first")
                    .clicked()
                {
                    action = RouteAction::StartHere;
                }
                if ui.button("Clear").clicked() {
                    self.waypoints.clear();
                    self.routes.clear();
                    self.error = None;
                    self.generation += 1;
                }
            });
            if self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("routing…");
                });
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e);
            }
            if self.routes.is_empty() {
                return;
            }
            ui.separator();
            for (i, r) in self.routes.iter().enumerate() {
                let label = format!(
                    "{}  {} · {}{}",
                    if i == 0 {
                        "Route".to_string()
                    } else {
                        format!("Alternative {i}")
                    },
                    crate::geo::fmt_distance(r.distance_m / 1000.0, readout.metric, 1),
                    fmt_duration(r.duration_s),
                    if r.summary.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", r.summary)
                    }
                );
                if ui.selectable_label(self.selected == i, label).clicked() {
                    self.selected = i;
                    self.generation += 1;
                }
            }
            if let Some((km, s)) = readout.remaining {
                ui.label(format!(
                    "From your position: {} left, about {}",
                    crate::geo::fmt_distance(km, readout.metric, 1),
                    fmt_duration(s)
                ));
            }
            ui.separator();
            ui.strong("Along this route");
            if readout.exposure.is_empty() {
                ui.label("No active warning or watch polygon crosses it right now.");
            }
            for (what, km, s) in readout.exposure {
                let text = if *km < 0.05 {
                    format!("Inside a {what} now")
                } else {
                    format!(
                        "Enters a {what} in {} (about {})",
                        crate::geo::fmt_distance(*km, readout.metric, 0),
                        fmt_duration(*s)
                    )
                };
                ui.colored_label(egui::Color32::from_rgb(240, 170, 90), text);
            }
            ui.weak(
                "Not a safety assessment: warnings can be issued over any road at any moment, and \
                 this only checks the polygons in effect now.",
            );
        });
        self.open = open;
        action
    }
}
