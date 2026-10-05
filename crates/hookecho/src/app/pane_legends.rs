//! The colour scales a pane shows over its map: the moment legend (a floating scale at the right
//! edge, or a strip on a phone) and the HRRR field key. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7); each method takes the locals its block read, under the same names.

use super::*;

impl HookEchoApp {
    /// The legend on a phone: a strip along the pane's bottom edge.
    pub(crate) fn paint_phone_legend(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        prect: egui::Rect,
        idx: usize,
    ) {
        let view = &self.views[idx];
        if crate::platform::phone_layout() && view.show_legend && view.volume.is_some() {
            use crate::ui::phone_design::Legend;
            let (df, dl) = display_units(view.moment, &self.settings);
            let table = self.palettes.table(view.moment);
            // Clear of the pill, mode bar and rail above, and the timeline below. Station's bars
            // and sheet are docked around the map, so its scale only keeps off the edges.
            let (top, clear_bottom) = if self.phone_station() {
                // Full screen, the way back (the eye) sits in the top-right corner.
                (
                    if self.mobile_chrome_hidden {
                        76.0
                    } else {
                        10.0
                    },
                    10.0,
                )
            } else {
                (
                    chrome::phone_top(ui.ctx()) + 56.0 + chrome::MODE_BAR_H + 8.0,
                    132.0 + self.phone_nav_h(),
                )
            };
            match self.settings.phone_design.spec().legend {
                Legend::StripOnly => {}
                Legend::Vertical => ui::legend::draw_vertical(
                    painter,
                    egui::Rect::from_min_max(
                        egui::pos2(prect.left(), prect.top() + top),
                        egui::pos2(prect.right(), prect.bottom() - clear_bottom),
                    ),
                    view.moment,
                    table,
                    view.active_threshold(),
                    df,
                    dl,
                ),
                Legend::Box => ui::legend::draw_box(
                    painter,
                    prect,
                    prect.bottom() - clear_bottom + 12.0,
                    &format!("{} ({dl})", crate::products::name(view.moment, false)),
                    table,
                    view.moment,
                    df,
                ),
            }
        }
    }

    /// The moment's scale, floating over the pane's right edge.
    pub(crate) fn paint_legend(&self, painter: &egui::Painter, prect: egui::Rect, idx: usize) {
        // Streaming mode can take the scale off the picture (`Broadcast::legend`).
        let legend_allowed = !(self.obs_mode && !self.settings.broadcast.legend);
        let view = &self.views[idx];
        if view.show_legend && legend_allowed && !crate::platform::phone_layout() {
            // The moment's scale floats over this pane's right edge (no panel, no card) so the map
            // keeps the pixels; the field/wind ramps still need their cards. The WSV3 layout docks
            // this same scale under the ribbon, so drawing it here too would be the third copy.
            let wsv3_colorbar =
                self.settings.layout.is_ribbon() && !crate::platform::phone_layout();
            if view.volume.is_some() && !wsv3_colorbar {
                if let Some((table, _, units)) = self.product_legend(idx) {
                    ui::legend::draw_vertical(
                        painter,
                        prect,
                        view.moment,
                        &table,
                        None,
                        1.0,
                        &units,
                    );
                } else {
                    let (df, dl) = display_units(view.moment, &self.settings);
                    ui::legend::draw_vertical(
                        painter,
                        prect,
                        view.moment,
                        self.palettes.table(view.moment),
                        view.active_threshold(),
                        df,
                        dl,
                    );
                }
            }
            // The field cards stack down the pane's top-left corner, which in the full-overlay
            // chrome is where the search pill floats — the first card was drawn half under it.
            // Every pane ducks by the same amount rather than only the top row: in a 2x2 grid the
            // lower cards then sit a little further from their pane's edge, which nobody notices,
            // and the alternative is a rect comparison that has to know about window insets.
            let mut y = 48.0;
            // Whichever gridded layer the user actually sees on top — the last enabled one in
            // paint order — gets its scale keyed underneath. Without this, MESH/QPE/VIL and the
            // categorical classifications were unlabeled color.
            if let Some(top) = crate::render::FieldLayer::paint_order(&self.settings.field_order)
                .iter()
                .rev()
                .find(|l| {
                    view.fields_on.contains(l)
                        && crate::fielddiff::layer_ready(**l, self.diff_valid, self.compare_valid)
                })
            {
                use crate::render::FieldLayer as FL;
                if *top == FL::ModelDiff {
                    y += ui::legend::draw_diff(painter, prect, self.diff_field, self.diff_mode, y);
                } else if *top == FL::Ensemble {
                    y += ui::legend::draw_ensemble(
                        painter,
                        prect,
                        &self.ensemble,
                        y,
                        self.settings.temp_unit,
                    );
                } else if matches!(*top, FL::CompareA | FL::CompareB) {
                    let (label_a, label_b) = self.diff_field.pair();
                    let model = if view.swipe_compare {
                        format!("{label_a} A | B {label_b}")
                    } else if view.overlay_compare {
                        format!("{label_a} + 50% {label_b}")
                    } else if *top == FL::CompareA {
                        label_a.into()
                    } else {
                        label_b.into()
                    };
                    y += ui::legend::draw_compare_label(painter, prect, y, &model);
                    y += ui::legend::draw_field(
                        painter,
                        prect,
                        self.diff_field.source_layer(),
                        y,
                        self.settings.temp_unit,
                    );
                } else {
                    y += ui::legend::draw_field(painter, prect, *top, y, self.settings.temp_unit);
                }
            }
            // Wind particles carry their own scale — it isn't a FieldLayer, so it needs its own
            // call rather than a slot in DRAW_ORDER.
            if self.show_wind && self.wind.is_some() {
                ui::legend::draw_ramp(
                    painter,
                    prect,
                    &crate::render::field_ramps::WIND,
                    y,
                    self.settings.temp_unit,
                );
            }
        }
    }

    /// The key for an HRRR field shown on the active pane.
    pub(crate) fn paint_hrrr_key(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        prect: egui::Rect,
        idx: usize,
    ) {
        let view = &self.views[idx];
        if view.fields_on.contains(&crate::render::FieldLayer::Hrrr) {
            let valid = self
                .field_state_for(idx, crate::render::FieldLayer::Hrrr)
                .filter(|_| self.model_field_ready_for(idx, crate::render::FieldLayer::Hrrr))
                .and_then(|state| state.stamp.as_ref().map(|stamp| stamp.valid_time))
                .map(|v| crate::timefmt::fmt_date_clock(v, self.active_tz()))
                .unwrap_or_else(|| "loading…".to_string());
            let lead_min = if self.views[idx].models.hrrr_subhourly {
                self.views[idx].models.hrrr_fcst_min
            } else {
                u16::from(self.views[idx].models.hrrr_fcst_hour) * 60
            };
            let lead = crate::model_browser::format_lead(lead_min);
            let lead = lead.trim_start_matches('F');
            let model = if view.models.hrrr_subhourly {
                "HRRR 15-MIN".into()
            } else {
                view.models.refl_model.label().to_uppercase()
            };
            let text = format!("⚠ FORECAST {lead} — {model} MODEL, NOT OBSERVED — valid {valid}");
            let font = egui::FontId::proportional(13.0);
            let pad = egui::vec2(10.0, 4.0);
            // On a phone the sentence is wider than the screen ("...valid Sep 20, 6:" ran off the
            // right edge), so it wraps, and it sits in the lane under the search pill and clear of
            // the control column instead of over the status bar. Desktop keeps the one-line strip
            // along the top.
            let phone = crate::platform::phone_layout();
            let (wrap, center_x, phone_y) = if phone {
                let (gutter_l, gutter_r) = self.phone_gutters();
                let left = prect.left() + crate::ui::m3::SP_3 + gutter_l;
                let right = prect.right() - crate::ui::m3::SP_3 - gutter_r;
                (
                    (right - left - pad.x * 2.0).max(120.0),
                    (left + right) / 2.0,
                    prect.top() + chrome::phone_top(ui.ctx()) + 56.0 + chrome::MODE_BAR_H + 8.0,
                )
            } else {
                (f32::INFINITY, prect.center().x, 0.0)
            };
            let galley = painter.layout(text, font, egui::Color32::BLACK, wrap);
            // Desktop: centred 16 pt down, exactly where the one-line strip always sat.
            let top = if phone {
                phone_y
            } else {
                prect.top() + 16.0 - (galley.size().y / 2.0 + pad.y)
            };
            let rect = egui::Rect::from_min_size(
                egui::pos2(center_x - (galley.size().x + pad.x * 2.0) / 2.0, top),
                galley.size() + pad * 2.0,
            );
            painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(255, 170, 60));
            painter.galley(rect.min + pad, galley, egui::Color32::BLACK);
        }
    }
}
