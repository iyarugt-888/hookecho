//! The 3D map's app side: building and draining the radar volume, 3D loops that play frame for
//! frame, and the 3D controls. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Build the 3D reflectivity volume from the active pane and open the raymarch window.
    pub(crate) fn build_volume3d(&mut self) {
        if !self.volume3d_supported {
            self.toast(
                ToastKind::Error,
                "3D needs WebGPU — this browser fell back to WebGL, which has no 3D textures",
            );
            return;
        }
        self.show_3d = true;
        let Some(vol) = self.views[self.active].volume.as_mut() else {
            return;
        };
        // Rebuild once per (volume, tilt count), not once per open: resampling 192x192x48 is a
        // second of CPU, but a live volume gains sweeps for a minute after the first chunk and the
        // 3D grid has to grow with it or the storm stays decapitated.
        let chosen = self.vol3d.selected_elevs.clone();
        let key = (
            vol.name.clone(),
            VOL3D_N,
            vol.elevations.len(),
            chosen.iter().map(|e| e.to_bits()).collect::<Vec<_>>(),
        );
        if self.vol3d_key.as_ref() == Some(&key) || self.vol3d_rx.is_some() {
            return;
        }
        let mut sweeps = vol.reflectivity_tilts();
        if sweeps.is_empty() {
            return;
        }
        self.vol3d_key = Some(key);
        // The layer-by-layer list, and with tilts pulled out, only those tilts' beams.
        self.vol3d.layers = sweeps.iter().map(wxdata::level2::layer_summary).collect();
        let full_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
        if !chosen.is_empty() {
            sweeps.retain(|s| chosen.iter().any(|&e| (e - s.elevation_deg).abs() < 0.05));
        }
        let table = crate::colormap::effective_table(
            &self.palettes,
            Moment::Reflectivity,
            self.settings.theme,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.vol3d_rx = Some(rx);
        self.spawner.spawn(async move {
            // Off the UI thread: this is pure CPU on tens of MB and would drop a second of frames.
            let built = wxdata::task::blocking(move || {
                // The volume covers whatever range this scan actually reported, not a fixed
                // radius — an old hardcoded 150 km half-width was clipping every storm beyond
                // it out of the 3D volume entirely, which no clipping-plane slice can recover
                // since a slice can only cut into range the volume already contains.
                // The whole volume's extent either way, so pulling tilts out never moves the box.
                let half_km = full_km;
                let v3 = if chosen.is_empty() {
                    wxdata::volume3d::build(&sweeps, VOL3D_N, VOL3D_NZ, half_km, VOL3D_TOP_KM)?
                } else {
                    wxdata::volume3d::build_shells(
                        &sweeps,
                        VOL3D_N,
                        VOL3D_NZ,
                        half_km,
                        VOL3D_TOP_KM,
                        crate::render3d::BEAMWIDTH_DEG,
                    )?
                };
                let lut =
                    crate::colormap::bake_lut(&table, (v3.value_min, v3.value_max), None).to_vec();
                Some((
                    crate::render3d::Volume3dUpload {
                        data: crate::render3d::pack_rg8(&v3.data),
                        n: v3.n as u32,
                        nz: v3.nz as u32,
                        lut,
                        half_km: v3.half_km,
                        top_km: v3.top_km,
                        outside: 0.0,
                        value_range: None,
                    },
                    (v3.value_min, v3.value_max),
                ))
            })
            .await
            .ok()
            .flatten();
            if let Some(b) = built {
                let _ = tx.send(b);
            }
        });
    }

    /// Take a finished 3D volume, if the worker has one.
    pub(crate) fn drain_volume3d(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.vol3d_rx else { return };
        match rx.try_recv() {
            Ok((upload, range)) => {
                self.vol3d_pending = Some(upload);
                self.vol3d_range = range;
                self.vol3d_rx = None;
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            // The worker gave up (no sweeps survived the resample); allow another attempt.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.vol3d_rx = None;
                self.vol3d_key = None;
            }
        }
    }

    /// The 3D builds pane `idx` needs for volume `name` (a complete volume) that are neither
    /// cached nor known to be empty.
    pub(crate) fn loop3d_missing(&self, idx: usize, name: &str) -> Vec<JobKey> {
        let mut out = Vec::new();
        if let Some(k) = self.smooth_key_for(idx, name, 0, true) {
            if !self.loop3d[idx].smooth.contains(&k) {
                out.push(JobKey::Smooth(idx, k));
            }
        }
        if let Some(k) = self.iso_key_for(idx, name, 0) {
            if !self.loop3d[idx].iso.contains(&k) {
                out.push(JobKey::Iso(idx, k));
            }
        }
        out.retain(|k| !self.loop3d_jobs.came_up_empty(k));
        out
    }

    /// While a loop plays in 3D, build the Smooth volume and isosurface of the frames playback
    /// reaches next, from volumes already in the download cache, so each frame's 3D is ready
    /// when the playhead gets there (ROADMAP_NEW H8).
    pub(crate) fn prebuild_loop3d(&mut self, idx: usize, ctx: &egui::Context) {
        let tl = &self.views[idx].timeline;
        if !tl.playing || !self.views[idx].map_3d.enabled {
            return;
        }
        let ahead: Vec<String> = crate::loop3d::upcoming(tl, crate::loop3d::PREBUILD_AHEAD)
            .into_iter()
            .filter_map(|i| tl.frames.get(i).map(|id| id.name().to_string()))
            .collect();
        for name in ahead {
            if self.loop3d_jobs.running(idx) >= crate::loop3d::MAX_BUILDS_PER_PANE {
                break;
            }
            let missing = self.loop3d_missing(idx, &name);
            if missing.is_empty() {
                continue;
            }
            // Only frames already downloaded; the frame prefetch brings the rest in.
            let Some(scan) = self.scan_cache.peek(&name).cloned() else {
                continue;
            };
            for job in missing {
                if !self.loop3d_jobs.wants(&job)
                    || self.loop3d_jobs.running(idx) >= crate::loop3d::MAX_BUILDS_PER_PANE
                {
                    continue;
                }
                let sweeps = crate::loop3d::Sweeps::Scan(Arc::clone(&scan));
                match &job {
                    JobKey::Smooth(..) => {
                        let Some(spec) = self.smooth_spec(idx, true) else {
                            continue;
                        };
                        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                            crate::loop3d::Built::Smooth(crate::loop3d::build_smooth(sweeps, &spec))
                        });
                    }
                    JobKey::Iso(..) => {
                        let spec = self.iso_spec(idx);
                        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                            crate::loop3d::Built::Iso(crate::loop3d::build_iso(sweeps, &spec))
                        });
                    }
                }
            }
        }
    }

    /// Whether playback should wait for the next frame's 3D: it is downloaded and its build is
    /// running, and has not been running for long ([`crate::loop3d::HOLD_FOR_BUILD`]).
    pub(crate) fn next_frame_3d_pending(&self, idx: usize) -> bool {
        let tl = &self.views[idx].timeline;
        if !tl.playing || !self.views[idx].map_3d.enabled {
            return false;
        }
        let Some(next) = crate::loop3d::next_frame(
            tl.playhead,
            tl.frames.len(),
            tl.replay,
            tl.following,
            tl.loop_enabled,
            tl.live_window,
        ) else {
            return false;
        };
        let Some(name) = tl.frames.get(next).map(|id| id.name().to_string()) else {
            return false;
        };
        self.loop3d_missing(idx, &name).iter().any(|k| {
            self.loop3d_jobs
                .started(k)
                .is_some_and(|at| at.elapsed() < crate::loop3d::HOLD_FOR_BUILD)
        })
    }

    /// While the loop plays in 3D: how many of the frames it cycles through have their 3D built.
    pub(crate) fn loop3d_progress(&self, idx: usize) -> Option<(usize, usize)> {
        let tl = &self.views[idx].timeline;
        if !tl.playing || !self.views[idx].map_3d.enabled {
            return None;
        }
        let mut frames = crate::loop3d::upcoming(tl, crate::loop3d::PREBUILD_AHEAD);
        frames.push(tl.playhead);
        let total = frames.len();
        let built = frames
            .iter()
            .filter_map(|i| tl.frames.get(*i))
            .filter(|id| self.loop3d_missing(idx, id.name()).is_empty())
            .count();
        (self.smooth_key_for(idx, "", 0, true).is_some() || self.iso_key_for(idx, "", 0).is_some())
            .then_some((built, total))
    }

    pub(crate) fn map_3d_controls(&mut self, idx: usize, prect: egui::Rect, ctx: &egui::Context) {
        // Every layout now owns its live 2D/3D choice in permanent chrome: the phone mode bar,
        // Dock top bar, ribbon Tools group, or Minimal control column. Keep this detail window out
        // of the map until 3D is active, then show only the representation/camera controls those
        // selectors do not duplicate.
        // The workstation draws these controls in its own 3D view window, docked or floating.
        if !self.views[idx].map_3d.enabled || self.workstation_chrome() {
            return;
        }
        // On a phone the desktop's spot (276 pt in from the right edge) is where the search pill and
        // the hide-chrome eye live, so the window opened on top of both. Start it in the lane
        // under the pill, left of the control column — and under the forecast banner when one is
        // showing.
        let pos = if crate::platform::phone_layout() {
            let banner = if self.views[idx].timeline.forecast_hour().is_some() {
                52.0
            } else {
                0.0
            };
            egui::pos2(
                prect.left() + crate::ui::m3::SP_3 + self.phone_gutters().0,
                chrome::phone_top(ctx) + 56.0 + chrome::MODE_BAR_H + 8.0 + banner,
            )
        } else {
            prect.right_top() + egui::vec2(-276.0, 8.0)
        };
        // A real Window rather than a fixed-position Area: dragging its title bar and resizing
        // from a corner both come free this way, and (unlike the Area this used to be) egui
        // remembers where a user left it — `default_pos`/`default_width` only seed the very
        // first appearance.
        egui::Window::new("3D map")
            .id(egui::Id::new(("map_3d_controls", idx)))
            .order(egui::Order::Foreground)
            .default_pos(pos)
            .default_width(260.0)
            .min_width(220.0)
            .resizable(true)
            .collapsible(true)
            .frame(
                egui::Frame::popup(&ctx.style_of(ctx.theme()))
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ctx, |ui| self.map_3d_controls_body(idx, ui));
    }

    /// The 3D controls themselves — representation, camera, beam rise, floors, quality, slicing —
    /// everything the permanent 2D/3D selectors do not already give. The floating "3D map"
    /// window draws them, and so does the workstation's 3D view window.
    pub(crate) fn map_3d_controls_body(&mut self, idx: usize, ui: &mut egui::Ui) {
        let volume_supported = self.volume3d_supported;
        let smooth_info = self.smooth_vol_info[idx];
        let product_range = self.smooth_vol_range[idx];
        // Products usable in 3D: those that parse and give a value at a single gate.
        let products: Vec<(String, bool)> = self
            .settings
            .udp_products
            .iter()
            .map(|p| {
                let ok = p.compile().is_ok_and(|e| !e.uses_column());
                (p.name.clone(), ok)
            })
            .collect();
        let loop_progress = self.loop3d_progress(idx);
        // The 3D shown belongs to another scan while this one's build runs.
        let shown = self.shown_volume_key(idx).map(|(n, _)| n);
        let behind = shown.as_deref().is_some_and(|n| {
            self.smooth_vol_key[idx].as_ref().is_some_and(|k| {
                k.0 != n
                    && self.views[idx]
                        .map_3d
                        .representation
                        .smooth_moment()
                        .is_some()
            }) || self.iso_mesh[idx]
                .as_ref()
                .is_some_and(|(k, _)| k.0 != n && self.views[idx].map_3d.iso_enabled)
        });
        let moment = self.views[idx].moment;
        // Read before `view` borrows `self.views[idx]`: a different field of
        // `self`, but taken up front keeps the split obviously safe rather than
        // relying on the borrow checker's disjoint-field-capture analysis.
        let cappi_alt_km = self.cappi_alt_km;
        let view = &mut self.views[idx];
        if view.camera.bearing.abs() > 0.1 && ui.small_button("North ↑").clicked() {
            view.camera.bearing = 0.0;
        }
        // Each resampled representation only ever shows one moment's volume; if
        // the pane's 2D product moves off that moment, fall back to Observed
        // rather than keep showing a volume for a product no longer selected.
        let stale = view
            .map_3d
            .representation
            .smooth_moment()
            .is_some_and(|(m, _)| m != moment);
        if stale {
            view.map_3d.representation = Map3dRepresentation::ObservedSweeps;
        }
        // Wrapped: eight choices do not fit a docked column's width on one line, and an
        // unwrapped row pushed the column wider than its room, under the map.
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut view.map_3d.representation,
                Map3dRepresentation::ObservedSweeps,
                "Observed",
            )
            .on_hover_text("Every real Level II tilt; no synthetic sweeps");
            ui.add_enabled_ui(volume_supported && moment == Moment::Reflectivity, |ui| {
                ui.selectable_value(
                    &mut view.map_3d.representation,
                    Map3dRepresentation::SmoothVolume,
                    "Smooth",
                )
            })
            .response
            .on_hover_text("Regularized reflectivity volume");
            ui.add_enabled_ui(
                volume_supported && moment == Moment::CorrelationCoefficient,
                |ui| {
                    ui.selectable_value(
                        &mut view.map_3d.representation,
                        Map3dRepresentation::SmoothDebris,
                        "Debris",
                    )
                },
            )
            .response
            .on_hover_text(
                "Lofted low correlation coefficient — possible tornado debris \
                     (TDS). Brighter = lower CC, inverted from the usual CC scale.",
            );
            ui.add_enabled_ui(volume_supported && moment == Moment::SpectrumWidth, |ui| {
                ui.selectable_value(
                    &mut view.map_3d.representation,
                    Map3dRepresentation::SmoothSpectrumWidth,
                    "SW",
                )
            })
            .response
            .on_hover_text(
                "Regularized spectrum-width volume — shear and turbulence \
                     signatures, the same continuous fill as Smooth/Debris",
            );
            ui.add_enabled_ui(
                volume_supported && moment == Moment::DifferentialReflectivity,
                |ui| {
                    ui.selectable_value(
                        &mut view.map_3d.representation,
                        Map3dRepresentation::SmoothZdr,
                        "ZDR",
                    )
                },
            )
            .response
            .on_hover_text(
                "Regularized ZDR volume — ZDR columns above the melting level mark \
                     strong updrafts",
            );
            ui.add_enabled_ui(
                volume_supported && moment == Moment::SpecificDifferentialPhase,
                |ui| {
                    ui.selectable_value(
                        &mut view.map_3d.representation,
                        Map3dRepresentation::SmoothKdp,
                        "KDP",
                    )
                },
            )
            .response
            .on_hover_text("Regularized KDP volume — heavy rain and melting-hail cores");
            ui.add_enabled_ui(volume_supported && moment == Moment::Velocity, |ui| {
                ui.selectable_value(
                    &mut view.map_3d.representation,
                    Map3dRepresentation::SmoothVelocity,
                    "VEL",
                )
            })
            .response
            .on_hover_text(
                "Dealiased velocity volume, strongest wind along each line of sight in                  either direction: both halves of a couplet show, inbound and outbound in                  their own colours. Where strong inbound meets strong outbound the boundary                  takes one colour or the other.",
            );
            ui.add_enabled_ui(
                volume_supported && moment == Moment::Reflectivity && !products.is_empty(),
                |ui| {
                    ui.selectable_value(
                        &mut view.map_3d.representation,
                        Map3dRepresentation::SmoothProduct,
                        "User",
                    )
                },
            )
            .response
            .on_hover_text(
                "A user-defined product (Tools > User products) worked out at every gate of the \
                 scan and drawn as a volume, like any moment. Shown while the map is on \
                 reflectivity.",
            );
        });
        ui.add(
            egui::Slider::new(
                &mut view.camera.pitch,
                0.0..=crate::render::mercator::MAX_PITCH_DEG,
            )
            .text("Pitch")
            .suffix("°"),
        );
        ui.add(
            egui::Slider::new(&mut view.camera.bearing, -180.0..=180.0)
                .text("Bearing")
                .suffix("°"),
        );
        ui.add(
            egui::Slider::new(&mut view.map_3d.vertical_exaggeration, 1.0..=8.0)
                .text("Vertical")
                .suffix("×"),
        );
        ui.add(egui::Slider::new(&mut view.map_3d.opacity, 0.1..=1.0).text("Opacity"));
        // Isosurface (Phase H3): the pane's moment at one threshold, per-moment.
        ui.checkbox(&mut view.map_3d.iso_enabled, "Isosurface")
            .on_hover_text(
                "A surface where this product crosses a threshold: a 50 dBZ core, a 3 dB ZDR \
                 column, a low-CC debris pocket (low CC is inside). Velocity draws a pair: \
                 outbound at +threshold and inbound at -threshold, each in its palette colour.",
            );
        if view.map_3d.iso_enabled {
            let mi = Moment::ALL.iter().position(|m| *m == moment).unwrap_or(0);
            let (lo, hi) = moment.value_range();
            let lo = if moment == Moment::Velocity { 0.0 } else { lo };
            ui.add(
                egui::Slider::new(&mut view.map_3d.iso_values[mi], lo..=hi)
                    .text(if moment == Moment::Velocity {
                        "± threshold"
                    } else {
                        "Threshold"
                    })
                    // Velocity is stored in m/s whatever the display units are.
                    .suffix(if moment == Moment::Velocity {
                        " m/s"
                    } else {
                        ""
                    })
                    .max_decimals(2),
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut view.map_3d.iso_nested, "Nested shells")
                    .on_hover_text(
                        "Two more surfaces inside the first, one and two steps further in, the \
                         outer ones fainter: a 40/50/60 dBZ envelope around its core",
                    );
                if view.map_3d.iso_nested {
                    let span = (hi - lo).abs();
                    ui.add(
                        egui::DragValue::new(&mut view.map_3d.iso_steps[mi])
                            .range(span / 100.0..=span / 3.0)
                            .speed(span / 500.0)
                            .max_decimals(2)
                            .prefix("step "),
                    );
                }
            });
            ui.add(
                egui::Slider::new(&mut view.map_3d.iso_opacity, 0.1..=1.0).text("Surface opacity"),
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut view.map_3d.iso_lit, "Lit")
                    .on_hover_text("Shade by a light from the northwest so the shape reads");
                ui.checkbox(&mut view.map_3d.iso_smooth, "Smooth surface")
                    .on_hover_text(
                    "Display smoothing of the surface only; the radar values are never smoothed",
                );
            });
        }
        ui.checkbox(&mut view.map_3d.mrms_surface, "MRMS echo tops as a surface")
            .on_hover_text(
                "Draw a displayed MRMS echo-top layer (18/30/50/60 dBZ) at its height: translucent \
                 with an analysis grid over it, so the analysed MRMS surface never reads as the \
                 observed radar",
            );
        ui.checkbox(
            &mut view.map_3d.cloud_top_surface,
            "Satellite cloud tops as a surface",
        )
        .on_hover_text(
            "GOES cloud top height (NOAA's ACHA product, 10 km, every 5 minutes) drawn at its \
                 height: pale grey to white, higher is whiter, fainter than the MRMS surface and \
                 without its grid",
        );
        ui.checkbox(
            &mut view.map_3d.model_isotherms,
            "HRRR 0/-10/-20 °C surfaces",
        )
        .on_hover_text(
            "The HRRR's latest analysis of the 0, -10 and -20 °C heights (the hail-growth zone) \
                 as model surfaces: one colour per level with a dashed grid, the forecast look",
        );
        ui.checkbox(&mut view.map_3d.cell_columns, "Storm cells as columns")
            .on_hover_text(
                "Each SCIT storm cell from base to top, a dot at the height of its strongest echo, \
                 TVS red and meso yellow, with its past track on the ground (needs storm cells on)",
            );
        ui.checkbox(&mut view.map_3d.height_ruler, "Height ruler")
            .on_hover_text("A km MSL ruler standing at the view's centre, labelled every 4 km");
        ui.checkbox(&mut view.map_3d.terrain, "Terrain")
            .on_hover_text(
                "The ground as a shaded surface at its height (AWS terrain tiles), at the same \
                 vertical exaggeration as everything else",
            );
        ui.checkbox(&mut view.map_3d.beam_guides, "Beam guides")
            .on_hover_text(
                "Draw the radar's beam geometry: each tilt's cone as rings at 50-200 km (low tilts cyan, high magenta), the lowest and highest beams toward the view with their 0.95° beamwidth edges, and the antenna mast",
            );
        if view.map_3d.representation == Map3dRepresentation::ObservedSweeps {
            ui.add(
                egui::Slider::new(&mut view.map_3d.beam_rise, 0.0..=1.0)
                    .text("Beam rise")
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .on_hover_text(
                "How much of each tilt's real climb with range to draw. A beam \
                 genuinely rises as it travels, so at long range every tilt flares \
                 steeply upward and the volume reads as a stack of cones. Lower this \
                 to pull the far end of each sweep back down — most where the rise is \
                 largest — or take it to 0% to lay the sweeps flat like the 2D view. \
                 100% is true beam geometry.",
            );
            if moment == Moment::CorrelationCoefficient {
                // CC gets the anomaly ramp instead of a floor — see `CcAnomaly`.
                // The pane's 2D threshold is untouched and still editable under
                // "Product settings"; it just stops applying to this 3D view,
                // because a floor and an anomaly ramp disagree about which end of
                // the CC scale is worth showing.
                map_3d_cc_anomaly_controls(ui, &mut view.map_3d.cc_anomaly);
            } else {
                // Same `threshold_enabled`/`thresholds` the 2D "Product settings"
                // Threshold control edits — one gate, so turning it on here also
                // denoises the flat 2D view and vice versa, rather than a second
                // floor a user has to keep in sync with the first.
                let mi = moment.index();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut view.threshold_enabled[mi], "Denoise")
                        .on_hover_text(
                            "Hide everything below a value, so light rain and \
                                 noise don't clutter the 3D scan",
                        );
                    if view.threshold_enabled[mi] {
                        let (vmin, vmax) = moment.value_range();
                        let (unit_factor, unit_label) = display_units(moment, &self.settings);
                        let f = unit_factor as f64;
                        let t = view.thresholds[mi].get_or_insert((vmin + vmax) * 0.5);
                        ui.add(
                            egui::Slider::new(t, vmin..=vmax)
                                .custom_formatter(move |v, _| format!("{:.0}", v * f))
                                .custom_parser(move |s| s.parse::<f64>().ok().map(|x| x / f))
                                .suffix(unit_label),
                        );
                    }
                });
            }
            if moment == Moment::SpecificDifferentialPhase {
                ui.weak("KDP is derived; shown on the map plane.");
            }
            if !view.map_3d.observed_layers.is_empty() {
                let layers = view.map_3d.observed_layers.clone();
                ui::volume3d_window::layers_section(
                    ui,
                    ("map_3d_layers", idx),
                    &layers,
                    &mut view.map_3d.selected_layer_elevs,
                    moment.units(),
                    "Click a tilt to pull it out and see its stats — click more to compare several at once.",
                );
            }
        } else {
            ui.weak("Vertical and Opacity above shape the resampled volume.");
            if matches!(
                view.map_3d.representation,
                Map3dRepresentation::SmoothVolume
                    | Map3dRepresentation::SmoothDebris
                    | Map3dRepresentation::SmoothSpectrumWidth
                    | Map3dRepresentation::SmoothZdr
                    | Map3dRepresentation::SmoothKdp
                    | Map3dRepresentation::SmoothVelocity
                    | Map3dRepresentation::SmoothProduct
            ) {
                ui.checkbox(&mut view.map_3d.smooth_full_range, "Full range")
                    .on_hover_text(
                        "Off: the volume is cropped to the range holding 99% of \
                         the echo, which keeps its cells small. On: everything the \
                         radar reported, with coarser cells.",
                    );
                if let Some((cell_km, outside)) = smooth_info {
                    let mut line = format!("{cell_km:.2} km cells");
                    if outside > 0.0005 {
                        line.push_str(&format!(
                            " · {:.1}% of echo outside the box",
                            outside * 100.0
                        ));
                    }
                    ui.weak(line);
                }
            }
            if view.map_3d.representation == Map3dRepresentation::SmoothProduct {
                if view.map_3d.product.is_none() {
                    view.map_3d.product = products.iter().find(|p| p.1).map(|p| p.0.clone());
                }
                let shown = view
                    .map_3d
                    .product
                    .clone()
                    .unwrap_or_else(|| "(none)".into());
                egui::ComboBox::from_label("Product")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (name, ok) in &products {
                            ui.add_enabled_ui(*ok, |ui| {
                                ui.selectable_value(
                                    &mut view.map_3d.product,
                                    Some(name.clone()),
                                    name,
                                )
                            })
                            .response
                            .on_disabled_hover_text(
                                "A vertical/layer function has no value at a single gate, \
                                 so this product has no volume",
                            );
                        }
                    });
                match product_range {
                    Some((lo, hi)) => {
                        ui.weak(format!("Drawn from {lo:.2} (blue) to {hi:.2} (magenta)"));
                    }
                    None => {
                        ui.weak("Building the product volume");
                    }
                }
            }
            if behind {
                ui.weak("3D is still the previous scan's; building this one");
            }
            if view.map_3d.representation == Map3dRepresentation::SmoothVelocity {
                ui.weak(if view.srv {
                    format!(
                        "Storm-relative: {:.0} kt from {:.0}\u{b0} taken off (SRV)",
                        view.storm_speed_kt,
                        (view.storm_dir_deg + 180.0).rem_euclid(360.0)
                    )
                } else {
                    "Ground-relative (turn on SRV for storm-relative)".to_string()
                });
            }
            if let Some((built, total)) = loop_progress.filter(|(b, t)| b < t) {
                ui.weak(format!("Loop 3D: {built} of {total} frames built"))
                    .on_hover_text(
                        "Playback waits briefly for each frame's 3D, so the volume always matches \
                         the time shown. Loop frames use a smaller grid; pause to see \
                         the frame at full resolution.",
                    );
            }
            if view.map_3d.representation == Map3dRepresentation::SmoothDebris {
                map_3d_cc_anomaly_controls(ui, &mut view.map_3d.cc_anomaly);
            }
            // `SmoothDebris`'s inverted-CC volume has no floor of its own here —
            // low CC is the interesting case there, not high — so only the two
            // plain "high is interesting" representations get a Denoise row, each
            // against its own floor field so switching between them never carries
            // one moment's number into another's units.
            let denoise_field = match view.map_3d.representation {
                Map3dRepresentation::SmoothVolume => Some((
                    &mut view.map_3d.reflectivity_floor_dbz,
                    Moment::Reflectivity.value_range(),
                    " dBZ",
                )),
                Map3dRepresentation::SmoothSpectrumWidth => Some((
                    &mut view.map_3d.sw_floor_ms,
                    Moment::SpectrumWidth.value_range(),
                    " m/s",
                )),
                Map3dRepresentation::SmoothZdr => Some((
                    &mut view.map_3d.zdr_floor_db,
                    Moment::DifferentialReflectivity.value_range(),
                    " dB",
                )),
                Map3dRepresentation::SmoothKdp => Some((
                    &mut view.map_3d.kdp_floor_deg_km,
                    Moment::SpecificDifferentialPhase.value_range(),
                    " °/km",
                )),
                // A speed, either direction.
                Map3dRepresentation::SmoothVelocity => Some((
                    &mut view.map_3d.velocity_floor_ms,
                    (0.0, Moment::Velocity.value_range().1),
                    " m/s",
                )),
                Map3dRepresentation::SmoothProduct => product_range.map(|(lo, hi)| {
                    let floor = &mut view.map_3d.product_floor;
                    *floor = floor.clamp(lo, hi);
                    (floor, (lo, hi), "")
                }),
                Map3dRepresentation::SmoothDebris | Map3dRepresentation::ObservedSweeps => None,
            };
            let rep = view.map_3d.representation as usize;
            if let Some((floor, (lo, hi), suffix)) = denoise_field {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut view.map_3d.denoise_enabled, "Denoise")
                        .on_hover_text(
                            "Hide weak values below the floor so the \
                                 interesting structure stands alone",
                        );
                    if view.map_3d.denoise_enabled {
                        ui.add(egui::Slider::new(floor, lo..=hi).suffix(suffix));
                    }
                });
                // The value window's other end: with a ceiling too, only one band of values
                // stays, such as the 45-55 dBZ shell around a hail core.
                if view.map_3d.denoise_enabled {
                    let floor_now = *floor;
                    let ceiling = &mut view.map_3d.ceilings[rep];
                    ui.horizontal(|ui| {
                        let mut on = ceiling.is_some();
                        if ui
                            .checkbox(&mut on, "Ceiling")
                            .on_hover_text(
                                "Also hide values above this, keeping one band between the \
                                 floor and the ceiling",
                            )
                            .changed()
                        {
                            *ceiling = on.then_some(hi.min(floor_now + (hi - lo) * 0.2));
                        }
                        if let Some(top) = ceiling {
                            ui.add(egui::Slider::new(top, floor_now..=hi).suffix(suffix));
                        }
                    });
                }
                ui::volume3d_window::opacity_curve(
                    ui,
                    &mut view.map_3d.tf_curves[rep],
                    (lo, hi),
                    suffix,
                );
                let label = view.map_3d.representation.label();
                if ui::volume3d_window::presets_row(
                    ui,
                    &mut self.settings.volume3d_presets,
                    label,
                    floor,
                    &mut view.map_3d.denoise_enabled,
                    &mut view.map_3d.ceilings[rep],
                    &mut view.map_3d.tf_curves[rep],
                    &mut view.map_3d.preset_name,
                ) {
                    self.settings.save();
                }
            }
            ui.horizontal(|ui| {
                ui.label("Quality");
                for (label, steps) in crate::view::QUALITY_PRESETS {
                    ui.selectable_value(&mut view.map_3d.quality_steps, steps, label);
                }
            });
            egui::CollapsingHeader::new("Slice")
                .id_salt(("map_3d_slice", idx))
                .default_open(false)
                .show(ui, |ui| {
                    let [x0, x1, y0, y1, z0, z1] = &mut view.map_3d.clip;
                    map_3d_axis_slice(ui, "E-W", x0, x1);
                    map_3d_axis_slice(ui, "N-S", y0, y1);
                    map_3d_axis_slice(ui, "Up ", z0, z1);
                    if ui.button("Whole volume").clicked() {
                        view.map_3d.clip = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
                    }
                    ui.separator();
                    ui::volume3d_window::plane_controls(ui, &mut view.map_3d.plane);
                    ui::volume3d_window::cappi_marker_controls(
                        ui,
                        &mut view.map_3d.cappi_marker,
                        cappi_alt_km,
                    );
                });
        }
        ui.weak("Right-drag rotates · drag pans · wheel zooms");
        ui.weak("W/S tilt · Q/E rotate");
    }
}
