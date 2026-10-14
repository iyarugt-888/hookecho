//! "A better radar for this storm" (1010.md V8, suggestions.md §4.3).
//!
//! For the storm being followed, or failing that the selected one, a radar of the same network
//! whose beam at the storm is far lower than the current radar's
//! ([`wxdata::suitability::recommend`]) is offered with the heights that say why and a one-tap
//! switch. Never switched automatically. Dismissing it hides that suggestion until it changes:
//! another radar, or the user on another radar.
use super::*;

impl HookEchoApp {
    /// The storm a radar suggestion is for, and its label: the followed storm, else the selected.
    fn hint_storm(&self) -> Option<(Cell, String)> {
        let cell = match &self.follow_cell {
            Some((_, c, _)) => c.clone(),
            None => self.selected_storm()?,
        };
        let label = if cell.id.is_empty() {
            "this storm".to_owned()
        } else {
            cell.id.clone()
        };
        Some((cell, label))
    }

    /// The suggestion card, in the badge lane below the follow badge.
    pub(super) fn radar_hint(&mut self, ctx: &egui::Context) {
        let Some(site) = self.views[self.active].site.clone() else {
            return;
        };
        let Some((storm, label)) = self.hint_storm() else {
            return;
        };
        let Some(r) = wxdata::suitability::recommend(&site, storm.lon, storm.lat, 0.5) else {
            return;
        };
        let better = r.better.site.id;
        let key = (site.clone(), better.to_owned());
        if self.radar_hint_dismissed.as_ref() == Some(&key) {
            return;
        }
        // One badge row lower while the follow badge shows above it.
        let below = if self.follow_cell.is_some() || self.follow_notice.is_some() {
            34.0
        } else {
            0.0
        };
        let mut switch = false;
        let mut dismiss = false;
        self.badge_lane("radar_hint", below, ctx).show(ctx, |ui| {
            // Each line is one row; see the same line in `follow_badge`.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(190))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    let white = egui::Color32::WHITE;
                    ui.label(
                        egui::RichText::new(format!("Better radar for {label}: {better}"))
                            .color(white)
                            .strong(),
                    );
                    let dim = egui::Color32::from_white_alpha(210);
                    for (radar, c) in [(better, r.better), (site.as_str(), r.current)] {
                        ui.label(
                            egui::RichText::new(format!(
                                "{radar}: beam {:.1} km up at the storm, {:.0} km away",
                                c.beam_height_m / 1000.0,
                                c.distance_km,
                            ))
                            .small()
                            .color(dim),
                        );
                    }
                    ui.horizontal(|ui| {
                        switch = ui
                            .button(format!("Switch to {better}"))
                            .on_hover_text("Show this storm from the lower beam")
                            .clicked();
                        dismiss = ui
                            .small_button("Not now")
                            .on_hover_text("Hide until a different radar is better")
                            .clicked();
                    });
                });
        });
        if dismiss {
            self.radar_hint_dismissed = Some(key);
        }
        if switch {
            self.switch_radar_for_storm(better, &storm);
        }
    }

    /// The badge lane the follow badge and this card share, `below` points down it.
    ///
    /// The workstation keeps its tool windows (the Inspector) at the maps' top right, which is
    /// where the lane sits in the other layouts, so there it runs down the top centre of the maps
    /// instead. The phone puts it below the top glass bar and the chrome-hide button (at inset_top
    /// + 66, `app/mobile.rs`); the floating desktop chrome, below its right control column.
    pub(super) fn badge_lane(
        &self,
        id: &'static str,
        below: f32,
        ctx: &egui::Context,
    ) -> egui::Area {
        let area = egui::Area::new(id.into());
        if self.workstation_chrome() && self.maps_rect.is_positive() {
            let top = egui::pos2(
                self.maps_rect.center().x,
                self.maps_rect.top() + 8.0 + below,
            );
            return area.pivot(egui::Align2::CENTER_TOP).fixed_pos(top);
        }
        let y = if crate::platform::phone_layout() {
            let inset_top = (ctx.content_rect().top() - ctx.viewport_rect().top()).max(0.0);
            inset_top + 116.0
        } else {
            crate::ui::style::lane_right_badge_y(CONTROL_BUTTONS)
        };
        area.anchor(
            egui::Align2::RIGHT_TOP,
            egui::vec2(crate::ui::style::LANE_RIGHT_BADGE_X, y + below),
        )
    }

    /// Switch the active pane to `site`, keeping the camera on `storm`, and carry a follow and a
    /// selection over.
    pub(super) fn switch_radar_for_storm(&mut self, site: &str, storm: &Cell) {
        // The site change drops the selection with the old radar's table; the storm it was is
        // found again in the new radar's (`adopt_carried_selection`).
        if let Some(selected) = self.selected_storm() {
            self.selection_carry = Some((site.to_owned(), selected, Instant::now()));
        }
        let v = &mut self.views[self.active];
        v.site = Some(site.to_owned());
        // `sync_pane` would centre on the new radar; the storm is what is being watched.
        v.camera_placed = true;
        v.camera.center = crate::render::mercator::lonlat_to_world(storm.lon, storm.lat);
        if let Some((_, cell, since)) = self.follow_cell.take() {
            // The new radar numbers its storms itself, and the same ID there is another storm.
            // With no ID, `update_follow` adopts the new radar's cell nearest where this one's
            // motion puts it.
            let carried = Cell {
                id: String::new(),
                ..cell
            };
            self.follow_cell = Some((site.to_owned(), carried, since));
        }
    }

    /// Select, on the radar a selection was taken to, the storm it was: once that radar's own
    /// table is in, for about the same time (an archive's local cells for an archived scan, not
    /// live SCIT), the cell nearest where the storm's motion puts it, within 15 km — as a follow
    /// is carried. Its ID there is another radar's numbering, so nothing is matched by ID. Given
    /// up after two minutes without such a table, or when there is no storm that near.
    pub(super) fn adopt_carried_selection(&mut self) {
        const WITHIN_KM: f64 = 15.0;
        const SAME_TIME_MIN: i64 = 15;
        let Some((site, storm, since)) = self.selection_carry.clone() else {
            return;
        };
        let expired = since.elapsed().as_secs() > 120;
        let cells = self.active_storm_cells();
        let table_time = cells.iter().find_map(|c| c.time);
        let ready = self.storm_table_site() == Some(site.as_str())
            && table_time
                .zip(storm.time)
                .is_some_and(|(t, s)| (t - s).num_minutes().abs() <= SAME_TIME_MIN);
        if !ready {
            if expired {
                self.selection_carry = None;
            }
            return;
        }
        let minutes = table_time
            .zip(storm.time)
            .map_or(0.0, |(t, s)| (t - s).num_seconds() as f64 / 60.0);
        let at = match (storm.mvt_deg, storm.mvt_kt) {
            (Some(dir), Some(kt)) if kt > 0.0 => {
                let km = f64::from(kt) * 1.852 * minutes / 60.0;
                crate::geo::destination_point([storm.lon, storm.lat], f64::from(dir), km)
            }
            _ => [storm.lon, storm.lat],
        };
        let found = super::nearest_cell(cells, at[0], at[1], WITHIN_KM).cloned();
        self.selection_carry = None;
        if let Some(c) = found {
            self.select_storm_from(c, false);
        }
    }
}
