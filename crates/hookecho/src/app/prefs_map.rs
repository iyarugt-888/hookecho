//! Preferences → Map: the background, radar appearance, launch and product rows.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Basemap style, smoothing, the startup view and the offline chase pack — the map knobs you
    /// set once and forget. They used to be the toolbox's "Map" section; they now sit under the
    /// drawer's App group (and the mobile drawer's Advanced group), which is the only other place
    /// per-map state is edited.
    pub(crate) fn map_rows(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut ui::layer_options::UiActions,
    ) {
        use crate::settings::StartView;
        let chasepack = self.chasepack_ui();
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.spacing_mut().interact_size.y = 32.0;
        let current = self.views[self.active].basemap;
        ui.label(egui::RichText::new("Background").strong());
        let mut picked = None;
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.menu_button(format!("{}    Change…", current.label()), |ui| {
                let width = (ui.ctx().content_rect().width() - 48.0).clamp(220.0, 460.0);
                ui.set_min_width(width);
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        picked =
                            ui::basemap_picker::grid(ui, &mut self.tiles, current, &self.settings);
                    });
                if picked.is_some() {
                    ui.close();
                }
            })
            .response
            .on_hover_text("Choose a map style. Shortcut: Z cycles backgrounds.");
        });
        if let Some(style) = picked {
            self.set_basemap(style);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.label(egui::RichText::new("Radar appearance").strong());
        let mut smooth = self.settings.smooth_radar;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            for (label, value, hint) in [
                ("Crisp", false, "Show each radar gate with a sharp edge"),
                ("Smooth", true, "Blend neighboring radar gates"),
            ] {
                if ui
                    .add_sized(
                        egui::vec2(width, 34.0),
                        egui::Button::new(label)
                            .selected(smooth == value)
                            .corner_radius(9.0),
                    )
                    .on_hover_text(hint)
                    .clicked()
                {
                    smooth = value;
                }
            }
        });
        if smooth != self.settings.smooth_radar {
            self.settings.smooth_radar = smooth;
            for view in &mut self.views {
                view.smooth = smooth;
            }
        }
        ui.checkbox(
            &mut self.settings.live_scan_indicator,
            "Live sweep indicator",
        )
        .on_hover_text(
            "Show an animated ring and tilt-progress bar next to the scrubber's Live badge \
                 while a live chunk stream is actively updating this pane.",
        );
        ui.horizontal(|ui| {
            ui.label("Live sweep display:");
            ui.selectable_value(
                &mut self.settings.live_sweep_mode,
                crate::settings::LiveSweepMode::ContinuousComposite,
                "Continuous composite",
            )
            .on_hover_text("Keep the previous pass dimmed until each azimuth is rescanned.");
            ui.selectable_value(
                &mut self.settings.live_sweep_mode,
                crate::settings::LiveSweepMode::StrictCurrentSweep,
                "Current sweep only",
            )
            .on_hover_text("Hide all radial rows identified as the previous pass.");
        });
        let (view, settings) = (&mut self.views[self.active], &mut self.settings);

        // A download in flight stays above the disclosure — progress you can't find reads as a hang.
        if let Some((done, total, errors, mb)) = chasepack.progress {
            ui.separator();
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                1.0
            };
            ui.add(egui::ProgressBar::new(frac).text(format!("{done}/{total} tiles · {mb:.0} MB")));
            if errors > 0 {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 120, 60),
                    format!("{errors} failed"),
                );
            }
            if ui.button("Cancel download").clicked() {
                actions.cancel_chasepack = true;
            }
            return;
        }

        ui.separator();
        ui.label(egui::RichText::new("On launch").strong());
        {
            // Startup view: remember this site + camera as the launch position.
            if ui
                .add_enabled(
                    view.site.is_some(),
                    egui::Button::new("Start here next time"),
                )
                .on_hover_text("Open here (site + map position) on next launch")
                .clicked()
            {
                if let Some(site) = &view.site {
                    settings.start_view = Some(StartView {
                        site: site.clone(),
                        x: view.camera.center.0,
                        y: view.camera.center.1,
                        zoom: view.camera.zoom,
                    });
                }
            }
            if let Some(site) = settings.start_view.as_ref().map(|sv| sv.site.clone()) {
                let mut clear = false;
                ui.horizontal(|ui| {
                    ui.weak(format!("Starts at {site}"));
                    clear = ui.small_button("Reset").clicked();
                });
                if clear {
                    settings.start_view = None;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        ui.collapsing("Offline maps", |ui| {
            // Offline chase pack: pre-cache this view's basemap tiles so it renders with no signal.
            ui.separator();
            if !chasepack.packable {
                if cfg!(target_arch = "wasm32") {
                    ui.weak("Offline packs are available in the desktop and Android apps");
                } else {
                    ui.weak("Offline pack: pick a raster or vector basemap");
                }
            } else {
                ui.weak(format!(
                    "Offline pack: {} tiles ≈ {:.0} MB (z{}–{}, current view)",
                    chasepack.tiles, chasepack.mb, chasepack.z_lo, chasepack.z_hi
                ));
                ui.checkbox(&mut settings.pack_include_vector, "Include streets")
                    .on_hover_text(
                        "Pack vector street tiles beside raster imagery, so road names still \
                         render offline",
                    );
                ui.checkbox(&mut settings.pack_include_satellite, "Include satellite")
                    .on_hover_text(
                        "Pack satellite imagery beside the vector streets, so terrain still \
                         renders offline",
                    );
                ui.checkbox(&mut settings.pack_hires_dem, "High-detail terrain")
                    .on_hover_text(
                        "z12 terrain (~40 m/px) instead of z10 — sixteen times the tiles",
                    );
                let too_big = chasepack.mb > 2000.0;
                if ui
                    .add_enabled(!too_big, egui::Button::new("⬇ Download offline pack"))
                    .on_hover_text(
                        "Cache this view's basemap tiles to disk for offline use in the field",
                    )
                    .clicked()
                {
                    actions.download_chasepack = true;
                }
                if too_big {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 90, 90),
                        "Too large (>2 GB) — zoom in or narrow the view",
                    );
                }
            }
        });
    }

    /// Sidebar header: the site, what you're looking at, its tilt, and the per-product knobs.
    ///
    /// The product list itself is the tree's Radar category (with a plain-English blurb per row);
    /// this section owns everything about the *current* product — the tilt picker and the expert
    /// options that used to hide in the toolbox. All of it writes the same fields the hotkeys do.
    pub(crate) fn product_section(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut ui::layer_options::UiActions,
    ) {
        use crate::ui::a11y::Named as _;
        use crate::ui::style;
        let (moment, srv, tilt) = {
            let v = &self.views[self.active];
            (v.moment, v.srv, v.tilt)
        };
        let elevations = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.elevations.clone())
            .unwrap_or_default();
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "Pick a site".to_string());

        let pick: Option<(wxdata::level2::Moment, bool)> = None;
        let mut pick_tilt: Option<usize> = None;
        // Expert knobs for the product you're on, edited through locals so the popup closure
        // doesn't need `self`. They used to live in the toolbox's Product ▸ Options disclosure.
        let mut srv_from_cells = false;
        let mut dealias = self.settings.dealias_velocity;
        let mut precip_tint = self.settings.precip_tint;
        let mut srv_on = srv;
        let mi = moment.index();
        let (mut dir_deg, mut speed_kt) = {
            let v = &self.views[self.active];
            (v.storm_dir_deg, v.storm_speed_kt)
        };
        let mut thr_on = self.views[self.active].threshold_enabled[mi];
        let mut thr = self.views[self.active].thresholds[mi];
        let (vmin, vmax) = moment.value_range();
        let (unit_factor, unit_label) = display_units(moment, &self.settings);
        let health = self.radar_health();
        let (status, status_color) = ui::layers_panel::health_look(health.state());
        let product_rect = style::glass(ui, 250)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(egui_phosphor::regular::BROADCAST)
                            .size(24.0).color(crate::theme::accent(self.settings.theme)))
                        .min_size(egui::vec2(42.0, 42.0)).corner_radius(21.0))
                        .named("Choose the radar site").clicked() {
                        actions.open_site_dialog = true;
                    }
                    ui.vertical(|ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(&site).strong())
                            .frame(false)).on_hover_text("Choose the radar site").clicked() {
                            actions.open_site_dialog = true;
                        }
                        if let Some(site) = wxdata::sites::site_by_id(&site) {
                            ui.label(egui::RichText::new(format!("{}, {}", site.city, site.state)).size(12.0)
                                .color(ui.visuals().weak_text_color()));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("● {status}"))
                            .size(11.0).color(status_color))
                            .on_hover_text("Radar source freshness");
                    });
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(crate::products::name(moment, srv))
                    .size(style::FONT_TITLE).strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Tilt")
                            .size(style::FONT_SM)
                            .color(ui.visuals().weak_text_color()),
                    )
                    .on_hover_text("How high above the ground the beam is looking");
                    let selected = elevations
                        .get(tilt)
                        .map_or_else(|| "Loading…".to_string(), |a| format!("{a:.1}\u{b0}"));
                    egui::ComboBox::from_id_salt("tilt_picker")
                        .selected_text(selected)
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            for (i, angle) in elevations.iter().enumerate() {
                                if ui
                                    .selectable_label(i == tilt, format!("{angle:.1}\u{b0}"))
                                    .clicked()
                                {
                                    pick_tilt = Some(i);
                                }
                            }
                        });
                });
                egui::CollapsingHeader::new("Product settings")
                    .default_open(false)
                    .show(ui, |ui| {
                        if moment == wxdata::level2::Moment::Reflectivity {
                            ui.checkbox(&mut precip_tint, "Tint by precipitation type")
                                .on_hover_text(
                                    "Colour the echo blue where it is falling as snow and pink \
                                     where it is freezing rain or sleet, from the MRMS surface \
                                     type. Reflectivity alone cannot tell them apart.",
                                );
                        }
                        if moment == wxdata::level2::Moment::Velocity {
                            ui.checkbox(&mut dealias, "Dealias").on_hover_text(
                                "Unfold aliased velocity (region-based dealiasing)",
                            );
                            ui.checkbox(&mut srv_on, "Storm-relative");
                            if srv_on {
                                ui.horizontal(|ui| {
                                    ui.label("Motion:");
                                    ui.add(
                                        egui::DragValue::new(&mut dir_deg)
                                            .range(0.0..=359.0)
                                            .suffix("\u{b0}"),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut speed_kt)
                                            .range(0.0..=150.0)
                                            .suffix(" kt"),
                                    );
                                });
                                if ui
                                    .button("From storm cells")
                                    .on_hover_text(
                                        "Set motion to the SCIT storm-cell mean (needs L3 storm cells)",
                                    )
                                    .clicked()
                                {
                                    srv_from_cells = true;
                                }
                            }
                        }
                        // Threshold for the active moment. The slider value stays internal (m/s
                        // for velocity); display honors the Units setting.
                        let f = unit_factor as f64;
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut thr_on, "Threshold").on_hover_text(
                                "Hide everything below a value \u{2014} cuts light rain out of the picture",
                            );
                            if thr_on {
                                let t = thr.get_or_insert((vmin + vmax) * 0.5);
                                ui.add(
                                    egui::Slider::new(t, vmin..=vmax)
                                        .custom_formatter(move |v, _| format!("{:.0}", v * f))
                                        .custom_parser(move |s| {
                                            s.parse::<f64>().ok().map(|x| x / f)
                                        })
                                        .suffix(unit_label),
                                );
                            }
                        });
                    });
            })
            .response
            .rect;
        self.tour_anchors.product = Some(product_rect);

        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if self.settings.precip_tint != precip_tint {
            self.settings.precip_tint = precip_tint;
            self.settings.save();
        }
        self.settings.dealias_velocity = dealias;
        // A product row was clicked this frame: it already set the moment and SRV flag, so the
        // knob write-back must not put the pre-click values back.
        if pick.is_none() {
            let v = &mut self.views[self.active];
            v.srv = srv_on;
            v.storm_dir_deg = dir_deg;
            v.storm_speed_kt = speed_kt;
            v.threshold_enabled[mi] = thr_on;
            v.thresholds[mi] = thr;
        }
        if srv_from_cells {
            if let Some((dir, spd)) = self.scit_mean_motion() {
                let v = &mut self.views[self.active];
                v.storm_dir_deg = dir;
                v.storm_speed_kt = spd;
                v.srv = true;
            }
        }
    }
}
