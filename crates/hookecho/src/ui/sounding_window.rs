//! Skew-T / hodograph window for a point sounding. A simplified Skew-T (temperature + dewpoint
//! vs log-pressure, temperature skewed) beside a hodograph of the wind profile.
//!
//! The model profile comes from HRRR, RAP or the NAM 3 km nest (picked in the window). Two
//! profiles can share the plot: the model forecast (solid) and, when a radiosonde site is near
//! enough, the observed ascent from that site (dashed). Seeing them together is the point — the
//! model's idea of the atmosphere against a sample of the real one.

use wxdata::sounding::Sounding;

/// Width one `theme::stat_card` occupies: its fixed 108 pt plus frame margins and item spacing.
const CARD_W: f32 = 132.0;

pub struct SoundingWindow {
    pub open: bool,
    pub busy: bool,
    pub sounding: Option<Sounding>,
    pub error: Option<String>,
    /// The observed radiosonde ascent nearest the clicked point, and the station it came from.
    pub observed: Option<Sounding>,
    pub observed_station: String,
    /// Why there's no observed profile — no station in range, or the balloon didn't fly.
    pub observed_error: Option<String>,
    pub show_observed: bool,
    /// Forecast hour the user has dialled up; a change asks the app for a new profile.
    pub fh: u8,
    /// Set for one frame when `fh` changed, so the app refetches.
    pub refetch: bool,
    /// The analyst's own storm motion (u, v, m/s), set by clicking the hodograph, and the point
    /// it was set for; `None` uses the Bunkers right mover.
    pub storm_motion: Option<((f64, f64), (f64, f64))>,
    /// Which parcel the CAPE cards and the Skew-T trace show.
    pub parcel_kind: wxdata::sounding::ParcelKind,
    /// Which model the profile is read from; a change asks the app for a new profile.
    pub model: wxdata::sounding::SoundingModel,
    /// The same valid time from the model's previous cycle, drawn dotted when shown.
    pub previous: Option<Sounding>,
    pub previous_error: Option<String>,
    pub show_previous: bool,
    /// Set for one frame when the previous run is wanted and not yet fetched.
    pub want_previous: bool,
}

impl Default for SoundingWindow {
    fn default() -> Self {
        Self {
            open: false,
            busy: false,
            sounding: None,
            error: None,
            observed: None,
            observed_station: String::new(),
            observed_error: None,
            // On by default: an observed profile is the more trustworthy of the two, and hiding it
            // behind a toggle nobody finds would waste the fetch.
            show_observed: true,
            fh: 0,
            refetch: false,
            storm_motion: None,
            parcel_kind: Default::default(),
            model: Default::default(),
            previous: None,
            previous_error: None,
            // Off by default: it is another forty-odd range requests.
            show_previous: false,
            want_previous: false,
        }
    }
}

impl SoundingWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        tz: Option<wxdata::tz::Tz>,
        drawer: &mut crate::ui::drawer::Drawer,
    ) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let Some(window) = drawer.page_sized(
            ctx,
            "Point Sounding",
            &mut open,
            false,
            560.0,
            egui::Window::new("Point Sounding"),
        ) else {
            self.open = open;
            return;
        };
        let phone = crate::platform::phone_layout();
        window.show(ctx, |ui| {
            if phone {
                egui::ScrollArea::vertical().show(ui, |ui| self.body(ui, tz, true));
            } else {
                self.body(ui, tz, false);
            }
        });
        self.open = open;
    }

    /// The window's contents: header, indices, the observed-profile line and the two plots —
    /// side by side, or `stacked` where the width is a phone's or a dock's (the caller scrolls).
    pub fn body(&mut self, ui: &mut egui::Ui, tz: Option<wxdata::tz::Tz>, stacked: bool) {
        // The model, above everything else so it can still be changed after a failed fetch.
        ui.horizontal(|ui| {
            ui.weak("Model");
            for model in wxdata::sounding::SoundingModel::ALL {
                if ui
                    .add_enabled(
                        model.available(),
                        egui::Button::selectable(self.model == model, model.label()),
                    )
                    .on_disabled_hover_text(
                        "Desktop only: the browser build cannot decode RAP's JPEG 2000 files",
                    )
                    .on_hover_text(match model {
                        wxdata::sounding::SoundingModel::Hrrr => "HRRR, 3 km, hourly",
                        wxdata::sounding::SoundingModel::Rap => "RAP, 13 km, hourly",
                        wxdata::sounding::SoundingModel::NamNest => {
                            "NAM 3 km CONUS nest, every six hours, to 60 h"
                        }
                    })
                    .clicked()
                    && self.model != model
                {
                    self.model = model;
                    self.refetch = true;
                }
            }
        });
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(format!("fetching {} profile…", self.model.label()));
            });
            return;
        }
        if let Some(e) = &self.error {
            ui.colored_label(
                egui::Color32::from_rgb(230, 120, 120),
                format!("Sounding unavailable: {e}"),
            );
            return;
        }
        let Some(s) = &self.sounding else {
            ui.weak("Press Ctrl+K, pick \"Tool: Sounding\", then click a point on the map.");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{:.2}, {:.2}", s.lat, s.lon));
            ui.separator();
            // Forecast hour: same run, later in it. The valid time is what the chaser
            // actually cares about, so it is the label.
            if ui
                .add_enabled(self.fh > 0, egui::Button::new("◀"))
                .clicked()
            {
                self.fh = self.fh.saturating_sub(1);
                self.refetch = true;
            }
            ui.strong(format!("f{:02}", s.fh));
            if ui
                .add_enabled(self.fh < 60, egui::Button::new("▶"))
                .clicked()
            {
                self.fh += 1;
                self.refetch = true;
            }
            ui.weak(format!(
                "valid {}",
                crate::timefmt::fmt_date_clock(s.run + chrono::Duration::hours(s.fh as i64), tz)
            ));
            ui.separator();
            ui.weak(format!("run {}", crate::timefmt::fmt_date_clock(s.run, tz)));
            if let Some(sh) = s.bulk_shear_kt() {
                ui.separator();
                ui.label(format!("0–6 km shear ≈ {sh:.0} kt"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::ui::csv_buttons(
                    ui,
                    "sounding.csv",
                    "The indices, then the profile they came from",
                    || s.to_csv(),
                );
            });
        });
        // A storm motion set at another point does not apply to this one.
        let here = (s.lon, s.lat);
        if self.storm_motion.is_some_and(|(_, at)| at != here) {
            self.storm_motion = None;
        }
        let custom = self.storm_motion.map(|(m, _)| m);
        // Fixed-layer composite indices (feature FF): the numbers a chaser scans first.
        if let Some(mut ix) = s.indices() {
            // An analyst's storm motion changes the helicity and everything built on it.
            if let Some(m) = custom {
                if let (Some(h1), Some(h3)) = (s.srh_relative(1000.0, m), s.srh_relative(3000.0, m))
                {
                    let shear6_ms = ix.shear6_kt / 1.943_844;
                    ix.srh1 = h1;
                    ix.srh3 = h3;
                    ix.scp = wxdata::severe::scp(ix.sbcape, h3, shear6_ms);
                    ix.stp = wxdata::severe::stp(ix.sbcape, h1, shear6_ms, ix.lcl_m);
                    ix.ehi1 = wxdata::severe::ehi1(ix.sbcape, h1);
                }
            }
            // The parcel the energy cards describe: surface-based, mixed-layer or most unstable.
            use wxdata::sounding::ParcelKind;
            ui.horizontal(|ui| {
                ui.weak("Parcel");
                for kind in ParcelKind::ALL {
                    ui.selectable_value(&mut self.parcel_kind, kind, kind.short())
                        .on_hover_text(match kind {
                            ParcelKind::SurfaceBased => "Surface-based: the surface air as it is",
                            ParcelKind::MixedLayer => "Mixed-layer: the lowest 100 hPa mixed",
                            ParcelKind::MostUnstable => {
                                "Most unstable: the level in the lowest 300 hPa with the most CAPE"
                            }
                        });
                }
            });
            let parcel = s.parcel_of(self.parcel_kind).map(|(p, _)| p);
            let (cape_label, cin_label) = match self.parcel_kind {
                ParcelKind::SurfaceBased => ("SBCAPE", "SBCIN"),
                ParcelKind::MixedLayer => ("MLCAPE", "MLCIN"),
                ParcelKind::MostUnstable => ("MUCAPE", "MUCIN"),
            };
            let level = |v: Option<f64>| match v {
                Some(m) => format!("{m:.0} m"),
                None => "—".to_string(),
            };
            let mut cards = vec![
                (
                    cape_label,
                    parcel
                        .as_ref()
                        .map_or("—".to_string(), |p| format!("{:.0} J/kg", p.cape)),
                ),
                (
                    cin_label,
                    parcel
                        .as_ref()
                        .map_or("—".to_string(), |p| format!("{:.0} J/kg", p.cin)),
                ),
                (
                    "LCL",
                    parcel
                        .as_ref()
                        .map_or("—".to_string(), |p| format!("{:.0} m", p.lcl_m)),
                ),
                ("LFC", level(parcel.as_ref().and_then(|p| p.lfc_m))),
                ("EL", level(parcel.as_ref().and_then(|p| p.el_m))),
                ("SRH 0–1", format!("{:.0}", ix.srh1)),
                ("SRH 0–3", format!("{:.0}", ix.srh3)),
                ("SCP", format!("{:.1}", ix.scp)),
                ("STP", format!("{:.1}", ix.stp)),
                ("EHI 0–1", format!("{:.1}", ix.ehi1)),
            ];
            // Effective-layer forms — the same solve the gridded severe layers run per
            // cell, so the panel and the map now agree in method. Absent when the column
            // has no effective inflow layer, which is the honest answer for a capped one.
            let eff = wxdata::severe::effective_indices(s);
            if let Some(e) = eff {
                let opt = |v: Option<f64>, p: usize| match v {
                    Some(x) => format!("{x:.*}", p),
                    None => "—".to_string(),
                };
                cards.push(("ESRH", format!("{:.0}", e.esrh)));
                cards.push(("EBWD", opt(e.ebwd_kt, 0)));
                cards.push(("STP (eff)", opt(e.stp_eff, 1)));
            }
            // Moisture, downdraft potential, lapse rates and the hail-growth temperatures.
            let or_dash = |v: Option<f64>, f: &dyn Fn(f64) -> String| v.map_or("—".to_string(), f);
            cards.push((
                "PWAT",
                or_dash(s.pwat_mm(), &|v| format!("{v:.0} mm ({:.2}\")", v / 25.4)),
            ));
            cards.push(("DCAPE", or_dash(s.dcape(), &|v| format!("{v:.0} J/kg"))));
            cards.push((
                "LR 0–3 km",
                or_dash(s.lapse_rate_c_km(0.0, 3000.0), &|v| format!("{v:.1} °C/km")),
            ));
            cards.push((
                "LR 700–500",
                or_dash(s.lapse_rate_between_c_km(700.0, 500.0), &|v| {
                    format!("{v:.1} °C/km")
                }),
            ));
            for (label, t) in [
                ("0 °C", 0.0),
                ("−10 °C", -10.0),
                ("−20 °C", -20.0),
                ("−30 °C", -30.0),
            ] {
                cards.push((
                    label,
                    or_dash(s.isotherm_height_m(t), &|v| format!("{v:.0} m")),
                ));
            }
            // Chunked by hand rather than `horizontal_wrapped`: a `Frame` with `set_width`
            // — which is what `stat_card` is — doesn't participate in egui's wrapping, so
            // all seven stayed on one 900 pt line and dragged the whole window off both
            // edges of a phone screen.
            let per_row = ((ui.available_width() / CARD_W) as usize).max(1);
            for row in cards.chunks(per_row) {
                ui.horizontal(|ui| {
                    for (label, value) in row {
                        crate::theme::stat_card(ui, label, value);
                    }
                });
            }
            ui.weak(if eff.is_some() {
                "Fixed- and effective-layer forms from 10 mandatory levels — coarser than SPC mesoanalysis."
            } else {
                "Fixed-layer forms from 10 mandatory levels — no effective inflow layer in this column."
            });
            ui.horizontal_wrapped(|ui| match custom {
                Some((u, v)) => {
                    let (dir, kt) = motion_dir_kt(u, v);
                    ui.weak(format!(
                        "Storm motion: yours, from {dir:03.0}° at {kt:.0} kt — SRH, SCP, STP and EHI use it."
                    ));
                    if ui.small_button("Use Bunkers").clicked() {
                        self.storm_motion = None;
                    }
                }
                None => {
                    ui.weak("Storm motion: Bunkers right mover. Click the hodograph to set your own.");
                }
            });
        }
        // Observed ascent: a line about where it came from, and the toggle.
        // Wrapped, not a plain `horizontal`: this row is long ("Fort Worth, TX (72249)
        // 28 Jul 12Z · …") and a non-wrapping row sets the window's minimum width, which
        // on a phone pushed the whole window off both screen edges.
        ui.horizontal_wrapped(|ui| match (&self.observed, &self.observed_error) {
            (Some(o), _) => {
                ui.checkbox(&mut self.show_observed, "Observed (RAOB)");
                ui.weak(format!(
                    "{} · {}",
                    self.observed_station,
                    crate::timefmt::fmt_date_clock(o.run, tz)
                ));
            }
            (None, Some(e)) => {
                ui.weak(format!("Observed sounding: {e}"));
            }
            (None, None) => {
                ui.weak("Observed sounding: fetching\u{2026}");
            }
        });
        let observed = self
            .show_observed
            .then_some(self.observed.as_ref())
            .flatten();
        // The model's previous cycle at the same valid time, fetched when first asked for.
        ui.horizontal_wrapped(|ui| {
            if ui
                .checkbox(&mut self.show_previous, "Previous run")
                .on_hover_text(
                    "The model's previous cycle at the same valid time, dotted: how the \
                     forecast changed between cycles",
                )
                .changed()
                && self.show_previous
                && self.previous.is_none()
            {
                self.want_previous = true;
            }
            if self.show_previous {
                match (&self.previous, &self.previous_error) {
                    (Some(p), _) => {
                        ui.weak(format!(
                            "run {} f{:02}",
                            crate::timefmt::fmt_date_clock(p.run, tz),
                            p.fh
                        ));
                    }
                    (None, Some(e)) => {
                        ui.weak(format!("unavailable: {e}"));
                    }
                    (None, None) => {
                        ui.weak("fetching\u{2026}");
                    }
                }
            }
        });
        let previous = self
            .show_previous
            .then_some(self.previous.as_ref())
            .flatten();
        ui.separator();
        // Phone: the fixed-width plots (300 + 240 px) side by side overflow the screen —
        // stack them vertically inside a scroll instead (fixed-width content overrides
        // phone_surface's max_width; same pattern as cell_window's grid).
        // Stacked, the caller scrolls the whole body: a scroll area of the plots alone, under
        // the header and indices, was left a sliver of height to show them in.
        let mut clicked = None;
        let chosen = s.parcel_of(self.parcel_kind);
        if stacked {
            skewt(ui, s, observed, previous, chosen.as_ref());
            ui.add_space(6.0);
            clicked = hodograph(ui, s, observed, previous, custom);
        } else {
            ui.horizontal(|ui| {
                skewt(ui, s, observed, previous, chosen.as_ref());
                clicked = hodograph(ui, s, observed, previous, custom);
            });
        }
        if let Some(m) = clicked {
            self.storm_motion = Some((m, here));
        }
    }
}

/// Simplified Skew-T: temperature (red) and dewpoint (green) plotted against log-pressure, with
/// temperature skewed 45° to the right (the classic emagram layout).
fn skewt(
    ui: &mut egui::Ui,
    s: &Sounding,
    observed: Option<&Sounding>,
    previous: Option<&Sounding>,
    parcel: Option<&(wxdata::sounding::Parcel, usize)>,
) {
    // Up to 300 px wide: narrower where it sits in a dock. The axes are laid out from `rect`.
    let w = ui.available_width().clamp(220.0, 300.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 380.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let grid = ui
        .visuals()
        .widgets
        .noninteractive
        .bg_stroke
        .color
        .gamma_multiply(0.6);

    // Vertical axis: log pressure 1000 (bottom) → 200 (top). Horizontal: temperature -40..40 C.
    let (p_bot, p_top) = (1000f64.ln(), 200f64.ln());
    let (t_min, t_max) = (-40.0f64, 40.0f64);
    let y_of = |hpa: f64| {
        let f = (hpa.ln() - p_bot) / (p_top - p_bot);
        rect.bottom() - 26.0 - f as f32 * (rect.height() - 40.0)
    };
    // Skew: shift temperature right as pressure decreases (higher up).
    let x_of = |temp_c: f64, hpa: f64| {
        let f = (temp_c - t_min) / (t_max - t_min);
        let skew = (p_bot - hpa.ln()) / (p_bot - p_top) * 60.0; // px of rightward skew at top
        rect.left() + 34.0 + f as f32 * (rect.width() - 44.0) + skew as f32
    };

    // Pressure gridlines + labels.
    for &hpa in &[1000.0, 850.0, 700.0, 500.0, 300.0, 200.0] {
        let y = y_of(hpa);
        p.hline(rect.x_range(), y, egui::Stroke::new(1.0, grid));
        p.text(
            egui::pos2(rect.left() + 3.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{hpa:.0}"),
            egui::FontId::proportional(9.0),
            ui.visuals().weak_text_color(),
        );
    }

    // `dash`: none for the forecast, long dashes for the observed ascent, dots for the
    // previous run.
    let trace = |src: &Sounding,
                 color: egui::Color32,
                 dash: Option<(f32, f32)>,
                 pick: &dyn Fn(&wxdata::sounding::SoundingLevel) -> f64| {
        let pts: Vec<egui::Pos2> = src
            .levels
            .iter()
            .filter(|l| l.pressure_hpa >= 195.0) // the plot stops at 200 hPa
            .map(|l| egui::pos2(x_of(pick(l), l.pressure_hpa), y_of(l.pressure_hpa)))
            .collect();
        if pts.len() < 2 {
            return;
        }
        let stroke = egui::Stroke::new(2.0, color);
        match dash {
            Some((on, off)) => p.extend(egui::Shape::dashed_line(&pts, stroke, on, off)),
            None => {
                p.add(egui::Shape::line(pts, stroke));
            }
        }
    };
    let (green, red) = (
        egui::Color32::from_rgb(120, 230, 120),
        egui::Color32::from_rgb(240, 90, 90),
    );
    // Observed underneath, so the forecast trace stays readable where they overlap.
    if let Some(o) = observed {
        trace(o, green.gamma_multiply(0.85), Some((5.0, 4.0)), &|l| {
            l.dewpt_c
        });
        trace(o, red.gamma_multiply(0.85), Some((5.0, 4.0)), &|l| l.temp_c);
    }
    if let Some(o) = previous {
        trace(o, green.gamma_multiply(0.6), Some((1.5, 3.0)), &|l| {
            l.dewpt_c
        });
        trace(o, red.gamma_multiply(0.6), Some((1.5, 3.0)), &|l| l.temp_c);
    }
    trace(s, green, None, &|l| l.dewpt_c);
    trace(s, red, None, &|l| l.temp_c);
    // The lifted parcel, and the CAPE it encloses: the shaded area *is* the number on the card.
    // Drawn from the level it starts at: a most-unstable parcel has no trace below its origin.
    if let Some((parcel, start)) = parcel {
        let pts: Vec<egui::Pos2> = s
            .levels
            .iter()
            .zip(&parcel.trace_c)
            .skip(*start)
            .filter(|(l, _)| l.pressure_hpa >= 195.0)
            .map(|(l, &t)| egui::pos2(x_of(t, l.pressure_hpa), y_of(l.pressure_hpa)))
            .collect();
        let env: Vec<egui::Pos2> = s
            .levels
            .iter()
            .skip(*start)
            .filter(|l| l.pressure_hpa >= 195.0)
            .map(|l| egui::pos2(x_of(l.temp_c, l.pressure_hpa), y_of(l.pressure_hpa)))
            .collect();
        // Positive-area shading, one quad per layer where the parcel is warmer than the air.
        for i in 1..pts.len().min(env.len()) {
            if pts[i - 1].x > env[i - 1].x && pts[i].x > env[i].x {
                p.add(egui::Shape::convex_polygon(
                    vec![env[i - 1], pts[i - 1], pts[i], env[i]],
                    egui::Color32::from_rgba_unmultiplied(240, 90, 90, 34),
                    egui::Stroke::NONE,
                ));
            }
        }
        if pts.len() >= 2 {
            p.add(egui::Shape::line(
                pts,
                egui::Stroke::new(1.2, egui::Color32::from_rgb(250, 200, 120)),
            ));
        }
    }
    p.text(
        rect.center_top() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_TOP,
        "Skew-T",
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

/// The direction a storm moves *from* (degrees) and its speed (kt), the way motions are quoted.
fn motion_dir_kt(u: f64, v: f64) -> (f64, f64) {
    let dir = (u.atan2(v).to_degrees() + 180.0).rem_euclid(360.0);
    (dir, (u * u + v * v).sqrt() * 1.943_844)
}

/// The hodograph's height bands (m AGL) and their colours: the conventional 0–1, 1–3, 3–6 and
/// 6–9 km layers, so where the curvature and the shear live reads at a glance.
const HODO_LAYERS: [(f64, f64, [u8; 3]); 4] = [
    (0.0, 1000.0, [235, 80, 80]),
    (1000.0, 3000.0, [90, 200, 90]),
    (3000.0, 6000.0, [235, 200, 60]),
    (6000.0, 9000.0, [90, 190, 235]),
];

/// Hodograph: wind (u, v) at each level, connected surface→top, in knots, coloured by height,
/// with the Bunkers storm motions marked. A click sets a storm motion; it is returned (m/s).
fn hodograph(
    ui: &mut egui::Ui,
    s: &Sounding,
    observed: Option<&Sounding>,
    previous: Option<&Sounding>,
    custom: Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let w = ui.available_width().clamp(200.0, 240.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, 380.0), egui::Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let grid = ui
        .visuals()
        .widgets
        .noninteractive
        .bg_stroke
        .color
        .gamma_multiply(0.6);
    let center = rect.center();
    let max_kt = 80.0f32;
    let r_px = (rect.width().min(rect.height()) / 2.0) - 20.0;
    // Range rings every 20 kt.
    for kt in [20.0, 40.0, 60.0, 80.0] {
        p.circle_stroke(center, r_px * kt / max_kt, egui::Stroke::new(1.0, grid));
    }
    p.line_segment(
        [
            egui::pos2(center.x - r_px, center.y),
            egui::pos2(center.x + r_px, center.y),
        ],
        egui::Stroke::new(1.0, grid),
    );
    p.line_segment(
        [
            egui::pos2(center.x, center.y - r_px),
            egui::pos2(center.x, center.y + r_px),
        ],
        egui::Stroke::new(1.0, grid),
    );

    let to_px = |u_ms: f64, v_ms: f64| {
        let (u_kt, v_kt) = (u_ms * 1.943_844, v_ms * 1.943_844);
        // East = +x, North = +y (up on screen).
        egui::pos2(
            center.x + r_px * (u_kt as f32 / max_kt),
            center.y - r_px * (v_kt as f32 / max_kt),
        )
    };
    // The observed hodograph is dashed and dimmer, drawn first so the forecast sits on top. Its
    // levels are thinned to the plotted layer: a 111-level radiosonde otherwise draws a hairball
    // of stratospheric wind above everything a chaser cares about.
    if let Some(o) = observed {
        let opts: Vec<egui::Pos2> = o
            .levels
            .iter()
            .filter(|l| l.pressure_hpa >= 250.0)
            .map(|l| to_px(l.u_ms, l.v_ms))
            .collect();
        if opts.len() >= 2 {
            p.extend(egui::Shape::dashed_line(
                &opts,
                egui::Stroke::new(
                    2.0,
                    egui::Color32::from_rgb(120, 180, 255).gamma_multiply(0.7),
                ),
                5.0,
                4.0,
            ));
        }
    }
    // The previous run's winds, dotted grey.
    if let Some(o) = previous {
        let pts: Vec<egui::Pos2> = o
            .levels
            .iter()
            .filter(|l| l.pressure_hpa >= 250.0)
            .map(|l| to_px(l.u_ms, l.v_ms))
            .collect();
        if pts.len() >= 2 {
            p.extend(egui::Shape::dashed_line(
                &pts,
                egui::Stroke::new(1.8, egui::Color32::from_gray(170)),
                1.5,
                3.0,
            ));
        }
    }
    // The profile sampled every 250 m through each height band, in the band's colour; above
    // 9 km, the remaining levels in grey.
    let heights = s.heights_m();
    for (z0, z1, [r, g, b]) in HODO_LAYERS {
        let mut pts = Vec::new();
        let mut z = z0;
        while z <= z1 + 1e-6 {
            match s.wind_at_height(z) {
                Some((u, v)) => pts.push(to_px(u, v)),
                None => break,
            }
            z += 250.0;
        }
        if pts.len() >= 2 {
            p.add(egui::Shape::line(
                pts,
                egui::Stroke::new(2.4, egui::Color32::from_rgb(r, g, b)),
            ));
        }
    }
    let upper: Vec<egui::Pos2> = s
        .levels
        .iter()
        .zip(&heights)
        .filter(|(l, &z)| z >= 9000.0 && l.pressure_hpa >= 200.0)
        .map(|(l, _)| to_px(l.u_ms, l.v_ms))
        .collect();
    if let (Some(top), Some((u9, v9))) = (upper.first(), s.wind_at_height(9000.0)) {
        let mut line = vec![to_px(u9, v9), *top];
        line.extend(upper.iter().skip(1));
        p.add(egui::Shape::line(
            line,
            egui::Stroke::new(1.6, egui::Color32::from_gray(150)),
        ));
    }
    if let Some(sfc) = s.levels.first() {
        p.circle_filled(to_px(sfc.u_ms, sfc.v_ms), 3.0, egui::Color32::WHITE); // surface marker
    }
    // Storm motions: Bunkers right (RM) and left (LM) movers, and the analyst's own.
    let mark = |at: egui::Pos2, text: &str, color: egui::Color32| {
        p.circle_stroke(at, 4.0, egui::Stroke::new(1.6, color));
        p.text(
            at + egui::vec2(6.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(10.0),
            color,
        );
    };
    if let Some((u, v)) = s.bunkers_rm() {
        mark(to_px(u, v), "RM", egui::Color32::from_rgb(240, 110, 110));
    }
    if let Some((u, v)) = s.bunkers_lm() {
        mark(to_px(u, v), "LM", egui::Color32::from_rgb(130, 170, 250));
    }
    if let Some((u, v)) = custom {
        let at = to_px(u, v);
        let c = egui::Color32::WHITE;
        let d = 5.0;
        p.line_segment(
            [at - egui::vec2(d, d), at + egui::vec2(d, d)],
            egui::Stroke::new(2.0, c),
        );
        p.line_segment(
            [at + egui::vec2(-d, d), at + egui::vec2(d, -d)],
            egui::Stroke::new(2.0, c),
        );
        p.text(
            at + egui::vec2(7.0, 7.0),
            egui::Align2::LEFT_TOP,
            "yours",
            egui::FontId::proportional(10.0),
            c,
        );
    }
    // The layer key along the bottom.
    let mut x = rect.left() + 8.0;
    for (label, (_, _, [r, g, b])) in ["0–1", "1–3", "3–6", "6–9 km"].iter().zip(HODO_LAYERS)
    {
        let galley = p.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(9.5),
            egui::Color32::from_rgb(r, g, b),
        );
        let wdt = galley.size().x;
        p.galley(
            egui::pos2(x, rect.bottom() - 16.0),
            galley,
            egui::Color32::WHITE,
        );
        x += wdt + 8.0;
    }
    p.text(
        rect.center_top() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_TOP,
        "Hodograph (kt)",
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    // A click anywhere on the plot sets the storm motion there.
    let click = response
        .on_hover_text("Click to set your own storm motion")
        .clicked()
        .then(|| ui.input(|i| i.pointer.interact_pos()))
        .flatten()?;
    let u_kt = (click.x - center.x) / r_px * max_kt;
    let v_kt = (center.y - click.y) / r_px * max_kt;
    Some((u_kt as f64 / 1.943_844, v_kt as f64 / 1.943_844))
}
