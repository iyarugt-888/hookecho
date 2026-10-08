//! The radar-suitability popup: nearby radars ranked by beam geometry at a clicked point, not
//! just distance — see `wxdata::suitability` for the math and its own honest limitations.

use wxdata::suitability::RadarCandidate;

/// Everything one click with the Radar suitability tool needs to show.
pub struct SuitabilityPopup {
    pub lon: f64,
    pub lat: f64,
    /// Nearest-first (see `wxdata::suitability::rank`'s own doc comment on why that is the same
    /// order beam height sorts to for one fixed elevation).
    pub candidates: Vec<RadarCandidate>,
}

/// Draws the popup. Returns `(keep_open, switch_to, compare_with)` — `switch_to` is
/// `Some(site_id)` the one frame a row's "Switch" is clicked, for the caller to apply as
/// `PaletteAction::SetSite`; `compare_with` is `Some(site_id)` the one frame a row's "Compare" is
/// clicked, for the caller to build a `coverage_compare` overlay between `current_site` and it
/// (ROADMAP_NEW C3's "radar coverage comparison between neighboring sites").
pub fn show(
    ctx: &egui::Context,
    popup: &SuitabilityPopup,
    current_site: Option<&str>,
    popovers: &mut crate::ui::popover::Popovers,
) -> (bool, Option<&'static str>, Option<&'static str>) {
    let mut open = true;
    let mut switch_to = None;
    let mut compare_with = None;
    popovers
        .card(
            ctx,
            "suitability_popup",
            egui::Window::new("Radar suitability").id(egui::Id::new("suitability_popup")),
        )
        .open(&mut open)
        .default_width(400.0)
        .max_height((ctx.content_rect().height() - 160.0).max(260.0))
        .vscroll(true)
        .resizable(true)
        .collapsible(false)
        .frame(
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(egui::Color32::from_rgb(17, 23, 31))
                .corner_radius(16)
                .inner_margin(18),
        )
        .show(ctx, |ui| {
            (switch_to, compare_with) = body(ui, popup, current_site);
        });
    (open, switch_to, compare_with)
}

fn body(
    ui: &mut egui::Ui,
    popup: &SuitabilityPopup,
    current_site: Option<&str>,
) -> (Option<&'static str>, Option<&'static str>) {
    ui.label(
        egui::RichText::new(format!("{:.4}, {:.4}", popup.lat, popup.lon))
            .size(11.0)
            .weak(),
    );
    ui.weak(
        "Ranked by beam height at this point (lowest first), which for one fixed tilt is the \
         same order distance sorts to — the numbers, not the order, are what a plain \"nearest \
         site\" list doesn't show.",
    );
    ui.add_space(6.0);
    let mut switch_to = None;
    let mut compare_with = None;
    egui::Grid::new("suitability_grid")
        .num_columns(6)
        .spacing([10.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.weak("Site");
            ui.weak("Distance");
            ui.weak("Beam height");
            ui.weak("Beam width");
            ui.weak("");
            ui.weak("");
            ui.end_row();
            for c in &popup.candidates {
                let is_current = current_site == Some(c.site.id);
                let label = format!("{} \u{2014} {}, {}", c.site.id, c.site.city, c.site.state);
                ui.label(egui::RichText::new(label).strong().color(if is_current {
                    egui::Color32::from_rgb(120, 200, 255)
                } else {
                    ui.visuals().text_color()
                }));
                ui.label(format!("{:.0} km", c.distance_km));
                ui.label(format!("{:.0} m", c.beam_height_m));
                ui.label(format!("{:.1} km", c.beam_width_km));
                if is_current {
                    ui.weak("current");
                    ui.weak("");
                } else {
                    if ui.small_button("Switch").clicked() {
                        switch_to = Some(c.site.id);
                    }
                    if current_site.is_some()
                        && ui
                            .small_button("Compare")
                            .on_hover_text(
                                "Paint a map overlay of which of this site and the current one \
                                 has the lower beam, point by point",
                            )
                            .clicked()
                    {
                        compare_with = Some(c.site.id);
                    }
                }
                ui.end_row();
            }
        });
    (switch_to, compare_with)
}
