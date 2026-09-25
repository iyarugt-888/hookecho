//! The ribbon chrome: a top ribbon of labelled control groups, the colour scale docked under it,
//! and a bottom status bar. Drawn only on desktop/web and only for `Layout::CommandRibbon`
//! (`Settings::layout.is_ribbon()`); `Layout::Wsv3` moved to the analyst workstation in `dock/`.
//! Android keeps its own touch chrome and the minimal layout keeps the floating pill + control
//! column in `overlay.rs`.
//!
//! Every control here funnels into the same `PaletteAction` / `UiActions` path the minimal chrome
//! and the command palette use, so the two layouts can never drift apart.

use super::*;
use crate::ui::wsv3;
use egui::{vec2, Align, Color32, Layout, RichText};

/// One fixed-width ribbon group: a label, then `add`'s content in a vertical scroll area, then a
/// hairline divider. Fixed width because `horizontal_wrapped` inside a group otherwise claims the
/// whole remaining ribbon width as its own and strands every group after it far to the right.
///
/// Scrollable rather than however many wrapped rows `add` happens to produce: the group's own
/// height is fixed (`wsv3::RIBBON_H`), and a plain `horizontal_wrapped`/manually-chunked row
/// layout has no way to reach content that wraps past that — it either overlapped the next group
/// or the map below the ribbon, and either way some pills were simply unreachable. A `ScrollArea`
/// costs nothing when everything already fits (no scrollbar appears), so every group gets it
/// rather than only the ones a bug report happened to name.
fn ribbon_group(ui: &mut egui::Ui, label: &str, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        vec2(w, wsv3::RIBBON_H - 6.0),
        Layout::top_down(Align::Min),
        |ui| {
            wsv3::group_label(ui, label);
            egui::ScrollArea::vertical()
                .id_salt(("ribbon_group_scroll", label))
                .max_height(ui.available_height())
                .show(ui, add);
        },
    );
    wsv3::vsep(ui);
}

/// Width of the tilt group: wide enough that every tilt pill plus the All and Follow-low pills fit in
/// two rows. At a fixed 250 px a 14-tilt volume wrapped to four rows in a group two rows tall, and the
/// upper tilts were reachable only by scrolling inside the ribbon. The ribbon itself scrolls
/// sideways when the groups outgrow the window.
fn tilt_group_width(tilts: usize) -> f32 {
    const PILL: f32 = 49.0; // a 42 px pill plus the item spacing
    let columns = (tilts + 2).div_ceil(2).max(5);
    (columns as f32 * PILL + 12.0).clamp(250.0, 620.0)
}

/// Phase B5: the VCP chip's popup — the full pattern description plus, per tilt, how many times
/// this volume revisits it and under what scheme (SAILS/MRLE). Everything here comes straight off
/// the decoded VCP message; nothing is inferred from how much of the volume has arrived.
fn scan_strategy_popup(ui: &mut egui::Ui, vcp: &str, cuts: &[wxdata::level2::TiltCuts]) {
    ui.set_min_width(230.0);
    ui.strong(if vcp.is_empty() { "Scan strategy" } else { vcp });
    if cuts.is_empty() {
        ui.weak("No volume loaded yet.");
        return;
    }
    ui.add_space(4.0);
    egui::Grid::new("wsv3_scan_strategy")
        .num_columns(3)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            ui.weak("Tilt");
            ui.weak("Cuts/vol");
            ui.weak("Scheme");
            ui.end_row();
            for c in cuts {
                ui.label(format!("{:.1}\u{b0}", c.elevation_deg));
                ui.label(c.cuts.to_string());
                let scheme = match (c.sails_cuts, c.mrle_cuts) {
                    (0, 0) => "\u{2014}".to_string(),
                    (s, 0) => format!("SAILS \u{d7}{s}"),
                    (0, m) => format!("MRLE \u{d7}{m}"),
                    (s, m) => format!("SAILS \u{d7}{s}, MRLE \u{d7}{m}"),
                };
                ui.label(scheme);
                ui.end_row();
            }
        });
}

/// A strip inside the bottom edge of the tilt pill the live chunk stream is currently updating —
/// filled by how far the current sweep has scanned (`chunk_index`/`chunks_in_sweep`) and pulsing
/// gently while drawn, so "the stream is alive and here's the tilt it's actually on right now" is
/// visible without a tooltip. Painted after `pill_sized` returns, directly over its own rect, so
/// it needs no layout space of its own that could collide with a pill wrapped onto the next row.
///
/// Skips the pulse (but keeps the fill) under [`crate::ui::motion::reduced`] — the same convention
/// the scrubber's own live-activity ring uses, for the same reason: the fill still answers "how
/// far", the pulse is only what answers "is it moving right now", and reduced motion asked to drop
/// the second question, not both.
pub(super) fn live_sweep_strip(
    ui: &mut egui::Ui,
    pill_rect: egui::Rect,
    p: wxdata::live::ScanProgress,
    accent: Color32,
) {
    if !ui.is_rect_visible(pill_rect) {
        return;
    }
    const HEIGHT: f32 = 3.0;
    const INSET: f32 = 3.0;
    let track = egui::Rect::from_min_max(
        egui::pos2(pill_rect.left() + INSET, pill_rect.bottom() - HEIGHT - 1.0),
        egui::pos2(pill_rect.right() - INSET, pill_rect.bottom() - 1.0),
    );
    let painter = ui.painter();
    painter.rect_filled(track, 1.5, Color32::from_black_alpha(90));
    let fraction = if p.chunks_in_sweep > 0 {
        (p.chunk_index as f32 / p.chunks_in_sweep as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // A sliver even at 0/N: an empty bar reads as "not live" at a glance, which is the one thing
    // this strip exists to say is false.
    let filled_w = track.width() * fraction.max(0.12);
    let fill = egui::Rect::from_min_size(track.min, egui::vec2(filled_w, track.height()));
    let alpha = if crate::ui::motion::reduced() {
        1.0
    } else {
        let t = ui.input(|i| i.time) as f32;
        ui.ctx().request_repaint();
        0.55 + 0.45 * (t * std::f32::consts::TAU * 0.5).sin().abs()
    };
    painter.rect_filled(fill, 1.5, accent.gamma_multiply(alpha));
}

impl HookEchoApp {
    /// The docked ribbon. Call on the eframe root `Ui`, before `chrome_rect` is captured, so the
    /// floating windows constrain to the map area below it.
    ///
    /// Collapsed (`ribbon_collapsed`), this draws nothing but the small corner button that brings
    /// it back — the panel itself is never created, so `chrome_rect` (captured right after this
    /// call returns) naturally grows to cover the space the ribbon would have reserved, handing
    /// the whole window to the map.
    pub(crate) fn wsv3_ribbon(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::ui::a11y::Named as _;
        if self.ribbon_collapsed {
            self.ribbon_collapse_button(ctx);
            return;
        }
        let accent = wsv3::WSV3_BLUE;
        let mut actions = ui::layer_options::UiActions::default();

        // --- reads, before the panel borrows `self` in a closure ---
        let (moment, srv, tilt) = {
            let v = &self.views[self.active];
            (v.moment, v.srv, v.tilt)
        };
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "Pick site".to_string());
        let city_state = wxdata::sites::site_by_id(&site)
            .map(|s| format!("{}, {}", s.city, s.state))
            .unwrap_or_default();
        let elevations = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.elevations.clone())
            .unwrap_or_default();
        let vcp = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.vcp.split(" (").next().unwrap_or_default().to_string())
            .unwrap_or_default();
        let vcp_full = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.vcp.clone())
            .unwrap_or_default();
        // One entry per tilt in `elevations` order, so index `i` below lines up with `elevations[i]`.
        let tilt_cuts = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| wxdata::level2::tilt_cuts(&v.scan))
            .unwrap_or_default();
        let show_live_indicator = self.settings.live_scan_indicator;
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(v, _, _, _)| *v == self.active);
        let live_progress = self.views[self.active].live_progress;
        let health = self.radar_health();
        let (health_txt, health_col) = ui::layers_panel::health_look(health.state());
        let panes = self.views.len();
        let pane_layout = self.pane_layout;
        let cur_tool = self.tool;
        let mut smooth = self.settings.smooth_radar;
        let mut follow_lowest_cut = self.views[self.active].follow_lowest_cut;
        let mut legend_on = self.views[self.active].show_legend;
        let layers_on = self.panel_open && !self.show_alert_panel;
        let alerts_on = self.panel_open && self.show_alert_panel;
        let basemap_on = self.basemap_open;
        let daynight_on = self.show_daynight;
        let map_3d = self.views[self.active].map_3d.enabled;
        let fields_on = self.views[self.active].fields_on.clone();
        let is_on = |l: crate::render::FieldLayer| fields_on.contains(&l);
        let model_sel = self.model_sel;
        let model_lead = self.model_lead_min();
        let model_valid = self
            .fields
            .get(&model_sel.layer())
            .and_then(|state| state.stamp.as_ref())
            .map(|stamp| stamp.valid_time);
        let tz_l = self.active_tz();
        let mode = self.ribbon_mode;
        let active_contours = self.active_contours.clone();

        let (disp_f, disp_l) = display_units(moment, &self.settings);
        let table = self.palettes.table(moment).clone();

        let mut pick_tilt: Option<usize> = None;
        let mut pick_panes: Option<usize> = None;
        let mut pick_pane_layout: Option<crate::workspace::PaneLayout> = None;
        let mut pick_mode: Option<crate::app::RibbonMode> = None;
        // Toggled this frame — several kinds can be active, so this isn't an exclusive pick.
        let mut pick_contour: Vec<crate::app::ContourKind> = Vec::new();
        let mut all_tilts = false;
        let mut open_command_search = false;
        let env_model = self.env_model;

        egui::Panel::top("wsv3_ribbon")
            .exact_size(wsv3::RIBBON_H + wsv3::COLORBAR_H)
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                let full = ui.max_rect();
                let ribbon_rect = egui::Rect::from_min_max(
                    full.min,
                    egui::pos2(full.right(), full.top() + wsv3::RIBBON_H),
                );
                wsv3::ribbon_gradient(ui.painter(), ribbon_rect);

                ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                egui::ScrollArea::horizontal()
                    .id_salt("wsv3_ribbon_groups")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .max_height(wsv3::RIBBON_H)
                    .show(ui, |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add_space(6.0);

                    // theme_plan.md §2.3: optionally pull this out of the ribbon entirely and
                    // draw it as a floating icon button over the map instead (below, once the
                    // ribbon panel is closed) — off by default, today's docked group unchanged.
                    if !self.settings.floating_search_button {
                        ribbon_group(ui, "Search", 136.0, |ui| {
                            if wsv3::pill(ui, "Search all", false, accent)
                                .on_hover_text("Search products, stations, tools, and UTC times (Ctrl+K)")
                                .clicked()
                            {
                                open_command_search = true;
                            }
                            ui.label(RichText::new("product · station · time").size(10.0).color(wsv3::STATUS_FG));
                        });
                    }

                    // ---- DATA (mode switch — WSV3 swaps its whole toolbar by data type) ----
                    ribbon_group(ui, "Data", 76.0, |ui| {
                        for m in crate::app::RibbonMode::ALL {
                            if wsv3::pill_sized(ui, m.label(), mode == m, accent, 62.0).clicked() {
                                pick_mode = Some(m);
                            }
                        }
                    });

                    let radar_mode = mode == crate::app::RibbonMode::Radar;
                    let model_mode = mode == crate::app::RibbonMode::Model;
                    let mrms_mode = mode == crate::app::RibbonMode::Mrms;

                    if radar_mode {
                    // ---- RADAR ----
                    ribbon_group(ui, "Radar", 208.0, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(egui_phosphor::regular::BROADCAST)
                                            .size(20.0)
                                            .color(accent),
                                    )
                                    .min_size(vec2(34.0, 34.0))
                                    .fill(wsv3::PILL_BG)
                                    .corner_radius(17.0),
                                )
                                .named("Choose the radar site")
                                .clicked()
                            {
                                actions.open_site_dialog = true;
                            }
                            ui.vertical(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(&site)
                                                .size(16.0)
                                                .strong()
                                                .color(wsv3::INK),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    actions.open_site_dialog = true;
                                }
                                ui.label(
                                    RichText::new(&city_state)
                                        .size(10.0)
                                        .color(Color32::from_white_alpha(130)),
                                );
                            });
                        });
                        ui.add_space(1.0);
                        for chunk in crate::products::PRODUCTS.chunks(4) {
                            ui.horizontal(|ui| {
                                for p in chunk {
                                    if wsv3::pill_sized(
                                        ui,
                                        p.short,
                                        p.moment == moment,
                                        accent,
                                        40.0,
                                    )
                                    .on_hover_text(p.blurb)
                                    .clicked()
                                    {
                                        actions.palette =
                                            Some(PaletteAction::SetMoment(p.moment, srv));
                                    }
                                }
                            });
                        }
                    });

                    // ---- TILT ----
                    ribbon_group(ui, "Tilt angle", tilt_group_width(elevations.len()), |ui| {
                        if elevations.is_empty() {
                            ui.label(
                                RichText::new("loading\u{2026}")
                                    .size(11.0)
                                    .color(wsv3::STATUS_FG),
                            );
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                for (i, a) in elevations.iter().enumerate() {
                                    // Marked whenever this VCP revisits the tilt mid-volume
                                    // (SAILS/MRLE) — otherwise identical to any other cut, so
                                    // nothing on screen said "you've already seen this angle
                                    // once this volume."
                                    let cut = tilt_cuts.get(i).copied();
                                    let repeated =
                                        cut.is_some_and(|c| c.sails_cuts > 0 || c.mrle_cuts > 0);
                                    let label = if repeated {
                                        format!("{a:.1}\u{b0}\u{2022}")
                                    } else {
                                        format!("{a:.1}\u{b0}")
                                    };
                                    let resp =
                                        wsv3::pill_sized(ui, &label, i == tilt, accent, 42.0);
                                    // The live chunk stream keeps scanning through the VCP
                                    // regardless of which tilt is on screen (`i == tilt`, the
                                    // pill's own highlight above) — this is a second, independent
                                    // answer to "which one is that", drawn as a strip inside the
                                    // pill's own bottom edge so it never needs layout space of its
                                    // own that could collide with a wrapped-to-the-next-row pill.
                                    if show_live_indicator
                                        && streaming
                                        && live_progress
                                            .is_some_and(|p| p.elevation_number.wrapping_sub(1) == i)
                                    {
                                        if let Some(p) = live_progress {
                                            live_sweep_strip(ui, resp.rect, p, accent);
                                        }
                                    }
                                    let resp = match cut {
                                        Some(c) if c.sails_cuts > 0 && c.mrle_cuts > 0 => resp
                                            .on_hover_text(format!(
                                                "Scanned {} times per volume: {} SAILS, {} MRLE",
                                                c.cuts, c.sails_cuts, c.mrle_cuts
                                            )),
                                        Some(c) if c.sails_cuts > 0 => resp.on_hover_text(format!(
                                            "Scanned {} times per volume ({} SAILS cut{})",
                                            c.cuts,
                                            c.sails_cuts,
                                            if c.sails_cuts == 1 { "" } else { "s" }
                                        )),
                                        Some(c) if c.mrle_cuts > 0 => resp.on_hover_text(format!(
                                            "Scanned {} times per volume ({} MRLE cut{})",
                                            c.cuts,
                                            c.mrle_cuts,
                                            if c.mrle_cuts == 1 { "" } else { "s" }
                                        )),
                                        _ => resp,
                                    };
                                    if resp.clicked() {
                                        pick_tilt = Some(i);
                                    }
                                }
                                if wsv3::pill(ui, "All \u{2317}", false, accent)
                                    .on_hover_text(
                                        "Four panes, one product, four tilts, cameras linked",
                                    )
                                    .clicked()
                                {
                                    all_tilts = true;
                                }
                                if wsv3::pill(ui, "Follow low", follow_lowest_cut, accent)
                                    .on_hover_text(
                                        "While following live, jump here the instant the lowest \
                                         tilt is rescanned (SAILS/MRLE) — don't wait for the \
                                         whole volume, or for whatever tilt is already selected.",
                                    )
                                    .clicked()
                                {
                                    follow_lowest_cut = !follow_lowest_cut;
                                }
                            });
                        }
                    });

                    // ---- VIEW ----
                    ribbon_group(ui, "View", 176.0, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("\u{25cf} {health_txt}"))
                                    .size(11.0)
                                    .color(health_col),
                            );
                            if !vcp.is_empty() {
                                let vcp_resp = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(&vcp).size(11.0).color(wsv3::STATUS_FG),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Scan strategy \u{2014} click for detail");
                                egui::Popup::menu(&vcp_resp)
                                    .show(|ui| scan_strategy_popup(ui, &vcp_full, &tilt_cuts));
                            }
                        });
                        wsv3::check(ui, "Smoothing", &mut smooth);
                        wsv3::check(ui, "Map legend", &mut legend_on);
                        ui.add_space(1.0);
                        ui.horizontal(|ui| {
                            for n in [1usize, 2, 3, 4, 6, 9]
                                .into_iter()
                                .filter(|n| *n <= crate::view::MAX_PANES)
                            {
                                if wsv3::pill_sized(
                                    ui,
                                    &format!("{n}\u{d7}"),
                                    panes == n,
                                    accent,
                                    27.0,
                                )
                                .on_hover_text(format!("{n}-pane layout"))
                                .clicked()
                                {
                                    pick_panes = Some(n);
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            for layout in crate::workspace::PaneLayout::ALL {
                                if wsv3::pill_sized(
                                    ui,
                                    layout.label(),
                                    pane_layout == layout,
                                    accent,
                                    54.0,
                                )
                                .on_hover_text(layout.description())
                                .clicked()
                                {
                                    pick_pane_layout = Some(layout);
                                }
                            }
                        });
                    });
                    } // radar_mode

                    if model_mode {
                    // ---- MODEL ----
                    // One model, its products, and its lead. A product pill is a toggle, so several
                    // can be on at once (HRRR reflectivity under GFS pressure); the model menu and
                    // stepper move the whole selection.
                    ribbon_group(ui, "Model", 300.0, |ui| {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("wsv3_model")
                                .selected_text(model_sel.model.label())
                                .width(112.0)
                                .show_ui(ui, |ui| {
                                    for family in crate::model_browser::Family::ALL {
                                        for m in crate::model_browser::BModel::ALL
                                            .into_iter()
                                            .filter(|m| m.family() == family)
                                        {
                                            if ui
                                                .selectable_label(model_sel.model == m, m.label())
                                                .on_hover_text(m.blurb())
                                                .clicked()
                                            {
                                                actions.palette = Some(PaletteAction::SetModel(m));
                                            }
                                        }
                                        if family != crate::model_browser::Family::Global {
                                            ui.separator();
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(model_sel.model.blurb());
                            if wsv3::pill(ui, "\u{2039}", false, accent).clicked() {
                                actions.palette = Some(PaletteAction::StepModelLead(-1));
                            }
                            ui.label(
                                RichText::new(crate::model_browser::format_lead(model_lead))
                                    .size(12.0)
                                    .strong()
                                    .color(wsv3::INK),
                            );
                            if wsv3::pill(ui, "\u{203a}", false, accent).clicked() {
                                actions.palette = Some(PaletteAction::StepModelLead(1));
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            for p in model_sel.model.products() {
                                if wsv3::pill(ui, p.label(), is_on(p.layer()), accent)
                                    .on_hover_text(p.blurb())
                                    .clicked()
                                {
                                    actions.palette = Some(PaletteAction::ToggleModelProduct(*p));
                                }
                            }
                        });
                        if let Some(v) = model_valid {
                            ui.label(
                                RichText::new(format!(
                                    "valid {}",
                                    crate::timefmt::fmt_clock(v, tz_l, false)
                                ))
                                .size(10.0)
                                .color(wsv3::STATUS_FG),
                            );
                        }
                    });

                    // ---- CONTOURS ----
                    // A checklist, not an exclusive pick: several model-contour fields (e.g. MSLP
                    // and CAPE) can be overlaid together.
                    ribbon_group(ui, "Contours", 140.0, |ui| {
                        // STP needs an LCL height only the HRRR surface file carries — see
                        // `ui::layer_options::stp_source`.
                        let stp_ok = matches!(env_model, wxdata::hrrr::Model::Hrrr);
                        egui::ComboBox::from_id_salt("wsv3_contours")
                            .selected_text(crate::app::summarize_contours(&active_contours))
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for ck in crate::app::ContourKind::ALL {
                                    if ck == crate::app::ContourKind::Off {
                                        continue;
                                    }
                                    if !stp_ok
                                        && matches!(
                                            ck,
                                            crate::app::ContourKind::Stp
                                                | crate::app::ContourKind::StpEff
                                        )
                                    {
                                        continue;
                                    }
                                    let mut on = active_contours.contains(&ck);
                                    if ui.checkbox(&mut on, ck.label()).changed() {
                                        pick_contour.push(ck);
                                    }
                                }
                            });
                    });

                    } // model_mode

                    if mrms_mode {
                    // ---- MRMS ----
                    ribbon_group(ui, "MRMS national", 268.0, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            use crate::render::FieldLayer as FL;
                            for (fl, label) in [
                                (FL::Mosaic, "Reflectivity"),
                                (FL::PrecipRate, "Precip rate"),
                                (FL::Qpe1h, "QPE 1 h"),
                                (FL::Qpe24h, "QPE 24 h"),
                                (FL::Rotation, "Rotation"),
                                (FL::AzShear, "AzShear"),
                                (FL::Mesh, "MESH hail"),
                                (FL::HailSwath, "Hail swath"),
                                (FL::PrecipType, "Precip type"),
                                (FL::FlashFlood, "FLASH"),
                                (FL::Lightning, "Lightning"),
                                (FL::EchoTops, "Echo tops"),
                            ] {
                                if wsv3::pill(ui, label, is_on(fl), accent).clicked() {
                                    actions.palette = Some(PaletteAction::ToggleField(fl));
                                }
                            }
                        });
                    });
                    } // mrms_mode

                    // ---- OVERLAYS ----
                    ribbon_group(ui, "Overlays", 108.0, |ui| {
                        if wsv3::pill(ui, "Layers", layers_on, accent).clicked() {
                            self.panel_open = !layers_on;
                            self.show_alert_panel = false;
                        }
                        if wsv3::pill(ui, "Alerts", alerts_on, accent).clicked() {
                            self.panel_open = !alerts_on;
                            self.show_alert_panel = true;
                        }
                        if wsv3::pill(ui, "Basemap", basemap_on, accent).clicked() {
                            self.basemap_open = !basemap_on;
                        }
                        if wsv3::pill(ui, "Day/Night", daynight_on, accent)
                            .on_hover_text(
                                "Night shading, the terminator, and a lat/lon graticule",
                            )
                            .clicked()
                        {
                            actions.palette =
                                Some(PaletteAction::ToggleOverlay(OverlayToggle::DayNight));
                        }
                    });

                    // ---- TOOLS ----
                    ribbon_group(ui, "Tools", 250.0, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (tool, label) in [
                                (MapTool::Interrogate, "Explore"),
                                (MapTool::GateInspector, "Gate inspector"),
                                (MapTool::RadarSuitability, "Radar suitability"),
                                (MapTool::Measure, "Measure"),
                                (MapTool::Marker, "Marker"),
                                (MapTool::CrossSection, "X-section"),
                                (MapTool::RegionStats, "Region stats"),
                                (MapTool::Sounding, "Sounding"),
                                (MapTool::Forecast, "Forecast"),
                            ] {
                                if wsv3::pill(ui, label, cur_tool == tool, accent).clicked() {
                                    actions.palette = Some(PaletteAction::Tool(tool));
                                }
                            }
                            if wsv3::pill(ui, "Cells", false, accent).clicked() {
                                actions.palette =
                                    Some(PaletteAction::OpenWindow(AppWindow::StormTable));
                            }
                            if wsv3::pill(ui, "3D map", map_3d, accent)
                                .on_hover_text("Tilt the live radar map into 3D")
                                .clicked()
                            {
                                actions.palette = Some(PaletteAction::ToggleMap3d);
                            }
                            if wsv3::pill(ui, "3D volume", false, accent).clicked() {
                                actions.palette =
                                    Some(PaletteAction::OpenWindow(AppWindow::Volume3d));
                            }
                        });
                    });

                    // ---- CAPTURE ----
                    ribbon_group(ui, "Capture", 118.0, |ui| {
                        if wsv3::pill(ui, "Share view", false, accent).clicked() {
                            actions.palette = Some(PaletteAction::CopyViewLink);
                        }
                        if wsv3::pill(ui, "Open in Windy", false, accent).clicked() {
                            actions.palette = Some(PaletteAction::OpenInWindy);
                        }
                        if wsv3::pill(ui, "Settings", false, accent).clicked() {
                            actions.palette = Some(PaletteAction::OpenWindow(AppWindow::Settings));
                        }
                        if wsv3::pill(ui, "Help", false, accent).clicked() {
                            actions.palette = Some(PaletteAction::OpenWindow(AppWindow::Help));
                        }
                    });
                    });
                    });

                // ---- docked colour scale ----
                let cb = egui::Rect::from_min_max(
                    egui::pos2(full.left(), full.top() + wsv3::RIBBON_H),
                    full.max,
                );
                wsv3::colorbar(ui.painter(), cb, &table, disp_f, disp_l);
            });
        // Drawn after the panel above (not before it), so it paints on top of the ribbon's own
        // background gradient rather than underneath it.
        self.ribbon_collapse_button(ctx);

        // theme_plan.md §2.3's floating search button, for whoever turned the docked ribbon
        // group off above. `default_pos` seeds its first location but, unlike an anchor, leaves
        // the Area movable; egui remembers the user's dragged position for the session.
        if self.settings.floating_search_button {
            let content = ctx.content_rect();
            let search_top = content.top() + wsv3::RIBBON_H + wsv3::COLORBAR_H;
            let search_bounds =
                egui::Rect::from_min_max(egui::pos2(content.left(), search_top), content.max);
            egui::Area::new(egui::Id::new("wsv3_floating_search"))
                .default_pos(egui::pos2(content.left() + 8.0, search_top + 8.0))
                .movable(true)
                .constrain_to(search_bounds)
                .show(ctx, |ui| {
                    crate::ui::style::glass(ui, 238).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("⠿")
                                    .size(15.0)
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .on_hover_cursor(egui::CursorIcon::Grab)
                            .on_hover_text("Drag to move the search button");
                            let btn = ui.add(
                                egui::Button::new(
                                    RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                                        .size(16.0)
                                        .color(accent),
                                )
                                .min_size(vec2(34.0, 34.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                            );
                            use crate::ui::a11y::Named as _;
                            if btn
                                .named("Search products, stations, tools, and UTC times (Ctrl+K)")
                                .clicked()
                            {
                                open_command_search = true;
                            }
                        });
                    });
                });
        }

        // --- apply ---
        if open_command_search {
            self.panel_open = true;
            self.show_alert_panel = false;
            self.sidebar_focus_search = true;
        }
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(n) = pick_panes {
            self.apply_palette(PaletteAction::SetPanes(n), ctx);
        }
        if let Some(layout) = pick_pane_layout {
            self.apply_palette(PaletteAction::SetPaneLayout(layout), ctx);
        }
        if all_tilts {
            self.apply_palette(PaletteAction::AllTilts, ctx);
        }
        if self.settings.smooth_radar != smooth {
            self.settings.smooth_radar = smooth;
            self.settings.save();
        }
        self.views[self.active].show_legend = legend_on;
        self.views[self.active].follow_lowest_cut = follow_lowest_cut;
        if let Some(m) = pick_mode {
            self.ribbon_mode = m;
        }
        for k in pick_contour {
            self.apply_palette(PaletteAction::SetContours(k), ctx);
        }
        self.apply_ui_actions(actions, ctx);
    }

    /// The bottom status bar: connection on the left, the valid time in the middle, the active
    /// camera's centre and zoom on the right.
    pub(crate) fn wsv3_status_bar(&mut self, root: &mut egui::Ui) {
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "\u{2014}".to_string());
        let age = self.views[self.active].volume.as_ref().map(|v| {
            let secs = (Utc::now() - v.time).num_seconds().max(0);
            humanize(secs)
        });
        let valid_dt = self.views[self.active]
            .timeline
            .current()
            .and_then(|id| id.date_time());
        let time_warning = {
            let mismatches = self.field_time_mismatches();
            mismatches
                .iter()
                .max_by_key(|(_, offset)| offset.num_seconds().abs())
                .map(|worst| {
                    let caption = format!(
                        "⚠ {} layer time mismatch{} (max {})",
                        mismatches.len(),
                        if mismatches.len() == 1 { "" } else { "es" },
                        crate::ui::data_inspector::offset_label(worst.1)
                    );
                    let details = mismatches
                        .iter()
                        .map(|(slug, offset)| {
                            format!(
                                "{}: {} from radar scan",
                                slug.slug(),
                                crate::ui::data_inspector::offset_label(*offset)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    (caption, details)
                })
        };
        let tz = self.active_tz();
        let valid = valid_dt
            .map(|d| crate::timefmt::fmt_clock(d, tz, false))
            .unwrap_or_default();
        let cam = self.views[self.active].camera;
        let (lon, lat) = crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
        let zoom = cam.zoom;

        egui::Panel::bottom("wsv3_status")
            .exact_size(wsv3::STATUS_H)
            .frame(egui::Frame::NONE.fill(wsv3::STATUS_BG))
            .show(root, |ui| {
                let lab = |ui: &mut egui::Ui, s: String| {
                    ui.label(RichText::new(s).size(11.0).color(wsv3::STATUS_FG));
                };
                ui.vertical(|ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(8.0);
                        lab(
                            ui,
                            match &age {
                                Some(a) => format!("{site}  \u{b7}  scan {a} ago"),
                                None => format!("{site}  \u{b7}  live only"),
                            },
                        );
                        if let Some((caption, details)) = &time_warning {
                            ui.separator();
                            ui.label(
                                RichText::new(caption)
                                    .size(11.0)
                                    .color(Color32::from_rgb(255, 197, 92)),
                            )
                            .on_hover_text(details);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            lab(ui, format!("{lat:.3}, {lon:.3}   \u{7c}   z{zoom:.1}"));
                            if !valid.is_empty() {
                                ui.separator();
                                lab(ui, valid);
                            }
                        });
                    });
                });
            });
    }

    /// The blue timestamp pill in the active pane's top-left corner.
    pub(crate) fn wsv3_timestamp(&mut self, ctx: &egui::Context) {
        let Some(d) = self.views[self.active]
            .timeline
            .current()
            .and_then(|id| id.date_time())
        else {
            return;
        };
        let text = crate::timefmt::fmt_date_clock(d, self.active_tz());
        egui::Area::new(egui::Id::new("wsv3_timestamp"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::LEFT_TOP, vec2(10.0, 10.0))
            .interactable(false)
            .show(ctx, |ui| {
                // Give the label room so it lays out on one line instead of wrapping to the
                // Area's shrink-wrapped width.
                ui.set_max_width(360.0);
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x22, 0x35, 0x5e))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x4a, 0x63, 0x9a)))
                    .corner_radius(5.0)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).size(12.5).color(Color32::WHITE));
                    });
            });
    }

    /// Hides/shows the ribbon for a full-window map view. Floats at the top-center edge rather
    /// than a corner: the left corner is the timestamp pill's spot (`wsv3_timestamp`) and the
    /// right is the OS window buttons' (`window_frame`) plus the docked colour scale's keepout —
    /// top-center is the one strip neither claims. Small and low-contrast on purpose, like a
    /// window's own resize grip, since it's a permanent fixture rather than an action to draw
    /// attention to.
    fn ribbon_collapse_button(&mut self, ctx: &egui::Context) {
        use crate::ui::a11y::Named as _;
        let (glyph, hint) = if self.ribbon_collapsed {
            (egui_phosphor::regular::CARET_DOWN, "Show the top bar (T)")
        } else {
            (
                egui_phosphor::regular::CARET_UP,
                "Hide the top bar for a full-window map view (T)",
            )
        };
        egui::Area::new(egui::Id::new("wsv3_ribbon_collapse"))
            .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 2.0))
            .interactable(true)
            .show(ctx, |ui| {
                let resp = ui
                    .add(
                        egui::Button::new(
                            RichText::new(glyph)
                                .size(11.0)
                                .color(Color32::from_white_alpha(160)),
                        )
                        .min_size(vec2(36.0, 12.0))
                        .fill(Color32::from_rgba_unmultiplied(0x14, 0x18, 0x1e, 190))
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius {
                            nw: 0,
                            ne: 0,
                            sw: 6,
                            se: 6,
                        }),
                    )
                    .named(hint);
                if resp.clicked() {
                    self.ribbon_collapsed = !self.ribbon_collapsed;
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_progress(chunk_index: usize, chunks_in_sweep: usize) -> wxdata::live::ScanProgress {
        wxdata::live::ScanProgress {
            elevation_number: 1,
            total_elevations: 14,
            elevation_angle_deg: 0.5,
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: (chunk_index.saturating_sub(1) as f64 * 360.0)
                / chunks_in_sweep.max(1) as f64,
            azimuth_end_deg: (chunk_index as f64 * 360.0) / chunks_in_sweep.max(1) as f64,
            chunk_index,
            chunks_in_sweep,
        }
    }

    /// The strip has to paint both pieces every time it's asked to: the dark track (so a viewer
    /// can tell "how much room is there" even at 0 progress) and the accent-coloured fill (so the
    /// pull request that wired the flag through the pill loop doesn't silently draw nothing).
    #[test]
    fn paints_a_track_and_a_fill() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 200.0),
            )),
            ..Default::default()
        };
        let pill_rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(42.0, 24.0));
        let output = ctx.run_ui(input, |ui| {
            live_sweep_strip(
                ui,
                pill_rect,
                sample_progress(1, 3),
                Color32::from_rgb(0, 120, 255),
            );
        });
        let rects: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) => Some(r.rect),
                _ => None,
            })
            .collect();
        assert_eq!(
            rects.len(),
            2,
            "expected a track and a fill rect: {rects:?}"
        );
    }

    /// At zero chunks scanned so far the fill still has to show *something* — an empty bar reads
    /// as "not live", which is exactly the state this strip exists to distinguish from "live, just
    /// hasn't scanned this sweep's first chunk yet".
    #[test]
    fn a_fresh_sweep_still_paints_a_visible_sliver() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 200.0),
            )),
            ..Default::default()
        };
        let pill_rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(42.0, 24.0));
        let output = ctx.run_ui(input, |ui| {
            live_sweep_strip(
                ui,
                pill_rect,
                sample_progress(0, 3),
                Color32::from_rgb(0, 120, 255),
            );
        });
        let widths: Vec<f32> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) => Some(r.rect.width()),
                _ => None,
            })
            .collect();
        assert!(
            widths.iter().any(|&w| w > 0.5),
            "expected a visible fill sliver even at zero progress: {widths:?}"
        );
    }
}

#[cfg(test)]
mod tilt_width_tests {
    use super::tilt_group_width;

    #[test]
    fn the_tilt_group_grows_with_the_volume_and_stays_within_bounds() {
        assert_eq!(tilt_group_width(0), 257.0_f32.clamp(250.0, 620.0));
        assert!(tilt_group_width(14) > tilt_group_width(8));
        assert!(tilt_group_width(8) >= tilt_group_width(4));
        // Two rows: every pill plus All and Follow-low fits in the columns it is given.
        for n in [4usize, 8, 9, 14, 19] {
            let cols = ((tilt_group_width(n) - 12.0) / 49.0).floor() as usize;
            assert!(cols * 2 >= n + 2, "{n} tilts in {cols} columns");
        }
        assert!(
            tilt_group_width(200) <= 620.0,
            "and it cannot swallow the ribbon"
        );
    }
}
