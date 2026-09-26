//! The Storms window (roadmap Q2's "denser product tables" and "dockable analyst panels"): every
//! SCIT cell in one compact table at dock width, ranked by the same severity score as the Storm
//! attributes window (`wxdata::cellscore`), so the two can never order the storms differently.
//!
//! A row click selects the storm — the map rings it and the Inspector's storm section shows it
//! (see `HookEchoApp::select_storm`), without switching this table's dock away; a double click
//! also centers the map on it. Headers sort; a
//! second click on the same header flips the direction. Rotation (TVS, meso) is a letter in the
//! last column, not a colour alone.

use super::*;
use crate::ui::a11y::Named as _;
use crate::ui::cells_window::{sorted_indices, SortCol};
use egui::{FontId, Rect, Sense, Stroke};
use egui_phosphor::regular as ph;

pub(super) const STORMS_W: f32 = 320.0;
const ROW_H: f32 = 22.0;

/// The table's columns: header, sort key, width.
const COLS: [(&str, SortCol, f32); 8] = [
    ("Sev", SortCol::Rank, 34.0),
    ("ID", SortCol::Id, 30.0),
    ("Rng", SortCol::Range, 34.0),
    ("dBZ", SortCol::MaxDbz, 32.0),
    ("Top", SortCol::Top, 30.0),
    ("VIL", SortCol::Vil, 30.0),
    ("SHI", SortCol::Posh, 34.0),
    ("Hail", SortCol::Hail, 34.0),
];

/// One cell's row text, in `COLS` order, then the rotation flags. Unknown values are a dash.
pub(super) fn row_cells(c: &wxdata::level3::Cell, score: Option<u8>) -> ([String; 8], String) {
    let o = |v: Option<f32>, p: usize| v.map_or_else(|| "\u{2014}".into(), |x| format!("{x:.p$}"));
    let cols = [
        score.map_or_else(|| "\u{2014}".into(), |s| s.to_string()),
        c.id.clone(),
        o(c.range_nm, 0),
        o(c.max_dbz, 0),
        o(c.top_kft, 0),
        o(c.vil, 0),
        c.posh
            .map_or_else(|| "\u{2014}".into(), |p| format!("{p}%")),
        c.hail_in
            .filter(|h| *h > 0.0)
            .map_or_else(|| "\u{2014}".into(), |h| format!("{h:.2}")),
    ];
    let mut flags = String::new();
    if c.tvs.as_ref().is_some_and(|t| !t.is_empty()) {
        flags.push('T');
    }
    if c.meso.as_ref().is_some_and(|m| !m.is_empty()) {
        flags.push('M');
    }
    (cols, flags)
}

impl HookEchoApp {
    pub(super) fn dock_storms(&mut self, host: Host<'_>) {
        if !self.dock.storms.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.storms.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.storms.collapsed;
        let cells: Vec<wxdata::level3::Cell> = self.active_storm_cells().to_vec();
        let archive = self.archive_bucket().is_some();
        let couplets: &[wxdata::rotation::CoupletHit] = match &self.couplet_cache {
            Some((_, (hits, ..))) => hits,
            None => &[],
        };
        let scores: Vec<u8> =
            wxdata::cellscore::score_all_explained(&cells, &self.probsevere, couplets)
                .iter()
                .map(|e| e.score)
                .collect();
        let (sort, desc) = (self.dock.storm_sort, self.dock.storm_desc);
        let order = sorted_indices(&cells, &scores, sort, desc);
        let selected = self.cell_popup.as_ref().map(|c| c.id.clone());
        let title = if cells.is_empty() {
            "Storms".to_string()
        } else {
            format!("Storms ({})", cells.len())
        };
        let list_h = (map_rect.height() - 90.0).clamp(140.0, 560.0);
        let mut header = ws::HeaderAction::None;
        let mut pick: Option<(usize, bool)> = None;
        let mut resort = None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_storms",
                place,
                width: STORMS_W,
                float_at: map_rect.left_top() + egui::vec2(12.0 + LEFT_WIDTH + 12.0, 60.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::TORNADO,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                if cells.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(ws::text(
                            if archive {
                                "Storm cells are live only: SCIT is not archived."
                            } else {
                                "No storm cells from this radar right now."
                            },
                            12.0,
                            t.text_dim,
                        ));
                    });
                    ui.add_space(8.0);
                    return;
                }
                // Header row: each column sorts.
                let (hr, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::hover());
                ui.painter().rect_filled(hr, 0.0, t.panel_hi);
                let mut x = hr.left() + 8.0;
                for (i, (label, key, w)) in COLS.iter().enumerate() {
                    let r = Rect::from_min_size(egui::pos2(x, hr.top()), egui::vec2(*w, ROW_H));
                    x += w;
                    let resp = ui
                        .interact(r, ui.id().with(("storm_col", i)), Sense::click())
                        .on_hover_text(match key {
                            SortCol::Rank => "Severity score, 0-100",
                            SortCol::Range => "Range from the radar, NM",
                            SortCol::Top => "Cell top, kft",
                            SortCol::Vil => "Water aloft, kg/m\u{b2}",
                            SortCol::Posh => "Probability of severe hail",
                            SortCol::Hail => "Max expected hail size, in",
                            _ => "",
                        });
                    let on = *key == sort;
                    let text = if on {
                        format!("{label}{}", if desc { "\u{25be}" } else { "\u{25b4}" })
                    } else {
                        label.to_string()
                    };
                    ui.painter().text(
                        r.left_center(),
                        egui::Align2::LEFT_CENTER,
                        text,
                        FontId::proportional(11.0),
                        if on { t.accent } else { t.text_dim },
                    );
                    if resp.clicked() {
                        resort = Some(*key);
                    }
                }
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("dock_storms_rows")
                    .auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(list_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (n, &i) in order.iter().enumerate() {
                        let c = &cells[i];
                        let (cols, flags) = row_cells(c, scores.get(i).copied());
                        let (r, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_H),
                            Sense::click(),
                        );
                        let on = selected.as_deref() == Some(c.id.as_str());
                        let p = ui.painter();
                        if on {
                            p.rect_filled(r, 0.0, t.accent_soft().gamma_multiply(0.6));
                            p.rect_filled(
                                Rect::from_min_size(r.min, egui::vec2(2.0, r.height())),
                                0.0,
                                t.accent,
                            );
                        } else if resp.hovered() {
                            p.rect_filled(r, 0.0, t.panel_hi);
                        } else if n % 2 == 1 {
                            p.rect_filled(r, 0.0, t.panel_hi.gamma_multiply(0.45));
                        }
                        let mut x = r.left() + 8.0;
                        for (k, text) in cols.iter().enumerate() {
                            // Severity and the hail columns take the warning colour when high,
                            // alongside the number itself.
                            let hot = match k {
                                0 => scores.get(i).is_some_and(|s| *s >= 60),
                                6 => c.posh.is_some_and(|p| p >= 50),
                                7 => c.hail_in.is_some_and(|h| h >= 1.0),
                                _ => false,
                            };
                            p.text(
                                egui::pos2(x, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                text,
                                FontId::monospace(11.0),
                                if hot {
                                    t.warn
                                } else if k == 1 {
                                    egui::Color32::WHITE
                                } else {
                                    t.text
                                },
                            );
                            x += COLS[k].2;
                        }
                        if !flags.is_empty() {
                            p.text(
                                egui::pos2(r.right() - 8.0, r.center().y),
                                egui::Align2::RIGHT_CENTER,
                                &flags,
                                FontId::monospace(11.0),
                                t.danger,
                            );
                        }
                        let resp = resp.named(&format!(
                            "Storm {}: severity {}{}",
                            c.id,
                            cols[0],
                            if flags.is_empty() {
                                String::new()
                            } else {
                                format!(", rotation {flags}")
                            }
                        ));
                        if resp.double_clicked() {
                            pick = Some((i, true));
                        } else if resp.clicked() {
                            pick = Some((i, false));
                        }
                    }
                });
                // A line under the table saying what the flags mean.
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), Sense::hover());
                ui.painter()
                    .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.line));
                ui.painter().text(
                    r.left_center() + egui::vec2(10.0, 1.0),
                    egui::Align2::LEFT_CENTER,
                    "T tornado vortex \u{b7} M mesocyclone \u{b7} double-click centers",
                    FontId::proportional(10.5),
                    t.text_faint,
                );
            },
        );
        self.dock.apply_header(DockWin::Storms, header);
        if let Some(key) = resort {
            if key == self.dock.storm_sort {
                self.dock.storm_desc = !self.dock.storm_desc;
            } else {
                self.dock.storm_sort = key;
                // Big numbers first, except names and range, which read best from the top down.
                self.dock.storm_desc = !matches!(key, SortCol::Id | SortCol::Range);
            }
        }
        if let Some((i, center)) = pick {
            let c = cells[i].clone();
            if center {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
                cam.zoom = cam.zoom.max(8.0);
            }
            self.select_storm_from(c, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_says_unknown_plainly_and_flags_rotation_in_letters() {
        let c = wxdata::level3::Cell {
            id: "Q4".into(),
            range_nm: Some(41.2),
            max_dbz: Some(63.0),
            posh: Some(70),
            hail_in: Some(1.75),
            meso: Some("M".into()),
            ..Default::default()
        };
        let (cols, flags) = row_cells(&c, Some(82));
        assert_eq!(cols[0], "82");
        assert_eq!(cols[1], "Q4");
        assert_eq!(cols[2], "41");
        assert_eq!(cols[4], "\u{2014}", "no top reported");
        assert_eq!(cols[6], "70%");
        assert_eq!(cols[7], "1.75");
        assert_eq!(flags, "M");
        let widths: f32 = COLS.iter().map(|c| c.2).sum();
        assert!(
            widths + 8.0 + 24.0 <= STORMS_W,
            "the columns and flags fit the dock"
        );
    }
}
