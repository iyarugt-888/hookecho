//! The WSV3 layout's docked chrome: a top ribbon of labelled control groups, the colour scale
//! docked under it, and a bottom status bar. Drawn only on desktop/web and only when
//! `Settings::layout` is `Wsv3`; Android keeps its own touch chrome and the minimal layout keeps
//! the floating pill + control column in `overlay.rs`.
//!
//! Every control here funnels into the same `PaletteAction` / `UiActions` path the minimal chrome
//! and the command palette use, so the two layouts can never drift apart.

use super::*;
use crate::ui::wsv3;
use egui::{vec2, Align, Color32, Layout, RichText};

/// One fixed-width ribbon group laid out top-down, followed by a hairline divider. Fixed width
/// because `horizontal_wrapped` inside a group otherwise claims the whole remaining ribbon width
/// as its own and strands every group after it far to the right.
fn ribbon_group(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        vec2(w, wsv3::RIBBON_H - 6.0),
        Layout::top_down(Align::Min),
        add,
    );
    wsv3::vsep(ui);
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

impl HookEchoApp {
    /// The docked ribbon. Call on the eframe root `Ui`, before `chrome_rect` is captured, so the
    /// floating windows constrain to the map area below it.
    pub(crate) fn wsv3_ribbon(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
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
        let health = self.radar_health();
        let (health_txt, health_col) = ui::layers_panel::health_look(health.state());
        let panes = self.views.len();
        let cur_tool = self.tool;
        let mut smooth = self.settings.smooth_radar;
        let mut legend_on = self.views[self.active].show_legend;
        let layers_on = self.panel_open && !self.show_alert_panel;
        let alerts_on = self.panel_open && self.show_alert_panel;
        let basemap_on = self.basemap_open;
        let daynight_on = self.show_daynight;
        let fields_on = self.views[self.active].fields_on.clone();
        let is_on = |l: crate::render::FieldLayer| fields_on.contains(&l);
        let hrrr_on = is_on(crate::render::FieldLayer::Hrrr);
        let hrrr_valid = self.hrrr_valid;
        let hrrr_sub = self.hrrr_subhourly;
        let tz_l = self.active_tz();
        let mode = self.ribbon_mode;
        let contour_kind = self.contour_kind;

        let (disp_f, disp_l) = display_units(moment, &self.settings);
        let table = self.palettes.table(moment).clone();

        let mut pick_tilt: Option<usize> = None;
        let mut pick_panes: Option<usize> = None;
        let mut pick_mode: Option<crate::app::RibbonMode> = None;
        let mut pick_contour: Option<crate::app::ContourKind> = None;
        let mut hrrr_hour = self.hrrr_fcst_hour;
        let mut hrrr_min = self.hrrr_fcst_min;
        let mut hrrr_sub_toggled = false;
        let mut all_tilts = false;
        // Global/env model source + lead, edited through locals so the sync loop refetches.
        let mut global_model = self.global_model;
        let mut global_hour = self.global_fcst_hour;
        let mut env_model = self.env_model;

        egui::Panel::top("wsv3_ribbon")
            .exact_size(wsv3::RIBBON_H + wsv3::COLORBAR_H)
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                let full = ui.max_rect();
                let ribbon_rect =
                    egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.top() + wsv3::RIBBON_H));
                wsv3::ribbon_gradient(ui.painter(), ribbon_rect);

                ui.spacing_mut().item_spacing = vec2(6.0, 3.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);

                    // ---- DATA (mode switch — WSV3 swaps its whole toolbar by data type) ----
                    ribbon_group(ui, 76.0, |ui| {
                        wsv3::group_label(ui, "Data");
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
                    ribbon_group(ui, 208.0, |ui| {
                        wsv3::group_label(ui, "Radar");
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
                    ribbon_group(ui, 250.0, |ui| {
                        wsv3::group_label(ui, "Tilt angle");
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
                            });
                        }
                    });

                    // ---- VIEW ----
                    ribbon_group(ui, 148.0, |ui| {
                        wsv3::group_label(ui, "View");
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
                            for n in [1usize, 2, 4] {
                                if wsv3::pill_sized(
                                    ui,
                                    &format!("{n}\u{d7}"),
                                    panes == n,
                                    accent,
                                    30.0,
                                )
                                .on_hover_text(format!("{n}-pane layout"))
                                .clicked()
                                {
                                    pick_panes = Some(n);
                                }
                            }
                        });
                    });
                    } // radar_mode

                    if model_mode {
                    // ---- MODEL SOURCE ----
                    ribbon_group(ui, 168.0, |ui| {
                        wsv3::group_label(ui, "Model");
                        ui.horizontal_wrapped(|ui| {
                            for gm in [
                                wxdata::global::GlobalModel::Gfs,
                                wxdata::global::GlobalModel::Ecmwf,
                                wxdata::global::GlobalModel::Gefs,
                                wxdata::global::GlobalModel::Gdps,
                            ] {
                                if wsv3::pill_sized(ui, gm.label(), global_model == gm, accent, 46.0)
                                    .clicked()
                                {
                                    global_model = gm;
                                }
                            }
                            for (em, label) in [
                                (wxdata::hrrr::Model::Hrrr, "HRRR"),
                                (wxdata::hrrr::Model::Rap, "RAP"),
                                (wxdata::hrrr::Model::NamNest, "NAM"),
                                (wxdata::hrrr::Model::Nam, "NAM12"),
                            ] {
                                if wsv3::pill_sized(ui, label, env_model == em, accent, 44.0)
                                    .on_hover_text(
                                        "Source for the CAPE / SRH / contour environment suite",
                                    )
                                    .clicked()
                                {
                                    env_model = em;
                                }
                            }
                        });
                        ui.add_space(1.0);
                        ui.horizontal(|ui| {
                            wsv3::group_label(ui, "Hour");
                            if wsv3::pill(ui, "\u{2039}", false, accent).clicked() {
                                global_hour = global_hour.saturating_sub(3);
                            }
                            ui.label(
                                RichText::new(format!("F+{global_hour}h"))
                                    .size(12.0)
                                    .strong()
                                    .color(wsv3::INK),
                            );
                            if wsv3::pill(ui, "\u{203a}", false, accent).clicked() {
                                global_hour = (global_hour + 3).min(120);
                            }
                        });
                    });

                    // ---- COLOR FILL ----
                    ribbon_group(ui, 210.0, |ui| {
                        wsv3::group_label(ui, "Color fill");
                        ui.horizontal_wrapped(|ui| {
                            use crate::render::FieldLayer as FL;
                            for (fl, label) in [
                                (FL::GlobalTemp2m, "2 m temp"),
                                (FL::GlobalDewpoint2m, "2 m dew"),
                                (FL::GlobalWind10m, "10 m wind"),
                                (FL::GlobalPrecip, "Precip"),
                                (FL::GlobalMslp, "MSLP"),
                                (FL::GlobalHeight500, "500 mb hgt"),
                                (FL::Cape, "CAPE"),
                                (FL::Srh, "SRH"),
                                (FL::UpdraftHelicity, "UH tracks"),
                                (FL::Smoke, "Smoke"),
                            ] {
                                if wsv3::pill(ui, label, is_on(fl), accent).clicked() {
                                    actions.palette = Some(PaletteAction::ToggleField(fl));
                                }
                            }
                        });
                    });

                    // ---- CONTOURS ----
                    ribbon_group(ui, 140.0, |ui| {
                        wsv3::group_label(ui, "Contours");
                        let mut sel = contour_kind;
                        egui::ComboBox::from_id_salt("wsv3_contours")
                            .selected_text(contour_kind.label())
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for ck in crate::app::ContourKind::ALL {
                                    ui.selectable_value(&mut sel, ck, ck.label());
                                }
                            });
                        if sel != contour_kind {
                            pick_contour = Some(sel);
                        }
                    });

                    // ---- FUTURE RADAR (HRRR) ----
                    ribbon_group(ui, 150.0, |ui| {
                        wsv3::group_label(ui, "Future radar");
                        if wsv3::pill(ui, "HRRR future", hrrr_on, accent)
                            .on_hover_text(
                                "HRRR composite-reflectivity forecast — future radar out to 18 h",
                            )
                            .clicked()
                        {
                            actions.palette = Some(PaletteAction::ToggleField(
                                crate::render::FieldLayer::Hrrr,
                            ));
                        }
                        if hrrr_on {
                            if wsv3::pill(ui, "15-min steps", hrrr_sub, accent)
                                .on_hover_text(
                                    "HRRR sub-hourly (wrfsubhf): scrub the tail in 15-minute \
                                     steps instead of whole hours",
                                )
                                .clicked()
                            {
                                hrrr_sub_toggled = true;
                            }
                            ui.horizontal(|ui| {
                                let dec = wsv3::pill(ui, "\u{2039}", false, accent).clicked();
                                let lead = if hrrr_sub {
                                    let h = hrrr_min / 60;
                                    let m = hrrr_min % 60;
                                    if h == 0 {
                                        format!("F+{m}m")
                                    } else if m == 0 {
                                        format!("F+{h}h")
                                    } else {
                                        format!("F+{h}h{m:02}m")
                                    }
                                } else {
                                    format!("F+{hrrr_hour}h")
                                };
                                ui.label(
                                    RichText::new(lead).size(12.0).strong().color(wsv3::INK),
                                );
                                let inc = wsv3::pill(ui, "\u{203a}", false, accent).clicked();
                                if hrrr_sub {
                                    if dec {
                                        hrrr_min = hrrr_min.saturating_sub(15).max(15);
                                    }
                                    if inc {
                                        hrrr_min = (hrrr_min + 15).min(18 * 60);
                                    }
                                } else {
                                    if dec {
                                        hrrr_hour = hrrr_hour.saturating_sub(1).max(1);
                                    }
                                    if inc {
                                        hrrr_hour = (hrrr_hour + 1).min(18);
                                    }
                                }
                            });
                            if let Some(v) = hrrr_valid {
                                ui.label(
                                    RichText::new(crate::timefmt::fmt_clock(v, tz_l, false))
                                        .size(10.0)
                                        .color(wsv3::STATUS_FG),
                                );
                            }
                        }
                    });
                    } // model_mode

                    if mrms_mode {
                    // ---- MRMS ----
                    ribbon_group(ui, 268.0, |ui| {
                        wsv3::group_label(ui, "MRMS national");
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
                    ribbon_group(ui, 108.0, |ui| {
                        wsv3::group_label(ui, "Overlays");
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
                    ribbon_group(ui, 224.0, |ui| {
                        wsv3::group_label(ui, "Tools");
                        ui.horizontal_wrapped(|ui| {
                            for (tool, label) in [
                                (MapTool::Interrogate, "Explore"),
                                (MapTool::Measure, "Measure"),
                                (MapTool::Marker, "Marker"),
                                (MapTool::CrossSection, "X-section"),
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
                            if wsv3::pill(ui, "3D", false, accent).clicked() {
                                actions.palette =
                                    Some(PaletteAction::OpenWindow(AppWindow::Volume3d));
                            }
                        });
                    });

                    // ---- CAPTURE ----
                    ribbon_group(ui, 118.0, |ui| {
                        wsv3::group_label(ui, "Capture");
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

                // ---- docked colour scale ----
                let cb = egui::Rect::from_min_max(
                    egui::pos2(full.left(), full.top() + wsv3::RIBBON_H),
                    full.max,
                );
                wsv3::colorbar(ui.painter(), cb, &table, disp_f, disp_l);
            });

        // --- apply ---
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(n) = pick_panes {
            self.apply_palette(PaletteAction::SetPanes(n), ctx);
        }
        if all_tilts {
            self.apply_palette(PaletteAction::AllTilts, ctx);
        }
        if self.settings.smooth_radar != smooth {
            self.settings.smooth_radar = smooth;
            self.settings.save();
        }
        self.views[self.active].show_legend = legend_on;
        if let Some(m) = pick_mode {
            self.ribbon_mode = m;
        }
        if let Some(k) = pick_contour {
            self.apply_palette(PaletteAction::SetContours(k), ctx);
        }
        // Model source / lead: the global sync loop refetches on any change to these, so a plain
        // write is enough there.
        self.global_model = global_model;
        self.global_fcst_hour = global_hour;
        if env_model != self.env_model {
            self.env_model = env_model;
            // Both CAPE/SRH and the contours are cut from this source — drop their fetch clocks so
            // the next frame reloads, and clear STP-family contours the coarser sources can't do
            // (no LCL height in the RAP / NAM-nest files; see wxdata::severe::fetch_grid).
            use crate::render::FieldLayer as FL;
            for l in [FL::Cape, FL::Srh] {
                if let Some(s) = self.fields.get_mut(&l) {
                    s.last_fetch = None;
                }
            }
            if !matches!(env_model, wxdata::hrrr::Model::Hrrr)
                && matches!(
                    self.contour_kind,
                    crate::app::ContourKind::Stp | crate::app::ContourKind::StpEff
                )
            {
                self.contour_kind = crate::app::ContourKind::Off;
            }
        }
        // The per-frame sync in `update` refetches when the selected lead changes; a no-op write
        // when the steppers weren't touched costs nothing.
        self.hrrr_fcst_hour = hrrr_hour;
        self.hrrr_fcst_min = hrrr_min;
        if hrrr_sub_toggled {
            self.hrrr_subhourly = !self.hrrr_subhourly;
            // The tail is now a different resolution — force a refetch at the new one.
            self.hrrr_fetched_hour = None;
            self.hrrr_fetched_min = None;
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
}
