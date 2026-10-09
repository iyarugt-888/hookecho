//! The 3D map's app side: building and draining the radar volume, 3D loops that play frame for
//! frame, and the 3D controls (moved out of `app.rs`, ROADMAP_2 §7; laid out as a property
//! panel since).

use super::*;
use crate::ui::workstation as ws;
use egui_phosphor::regular as ph;

impl HookEchoApp {
    /// The 3D builds pane `idx` needs for volume `name` (a complete volume) that are neither
    /// cached nor known to be empty.
    pub(crate) fn loop3d_missing(&self, idx: usize, name: &str) -> Vec<JobKey> {
        let Some(source) = self.cached_volume_key(idx, name) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(k) = self.smooth_key_for(idx, source.clone(), true) {
            if !self.loop3d[idx].smooth.contains(&k) {
                out.push(JobKey::Smooth(idx, k));
            }
        }
        if let Some(k) = self.iso_key_for(idx, source) {
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
                let source = match &job {
                    JobKey::Smooth(_, key) => &key.source,
                    JobKey::Iso(_, key) => &key.source,
                };
                let sweeps =
                    crate::loop3d::Sweeps::captured(Arc::clone(&scan), source.acquisition());
                match &job {
                    JobKey::Smooth(_, key) => {
                        let policy = key.source.policy;
                        let Some(spec) = self.smooth_spec(idx, true) else {
                            continue;
                        };
                        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                            crate::loop3d::Built::Smooth(crate::loop3d::build_smooth_covered(
                                sweeps, &spec, policy,
                            ))
                        });
                    }
                    JobKey::Iso(_, key) => {
                        let policy = key.source.policy;
                        let spec = self.iso_spec(idx);
                        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                            crate::loop3d::Built::Iso(crate::loop3d::build_iso_covered(
                                sweeps, &spec, policy,
                            ))
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
            .filter(|id| {
                self.cached_volume_key(idx, id.name())
                    .is_some_and(|source| {
                        self.smooth_key_for(idx, source.clone(), true)
                            .is_none_or(|k| self.loop3d[idx].smooth.contains(&k))
                            && self
                                .iso_key_for(idx, source)
                                .is_none_or(|k| self.loop3d[idx].iso.contains(&k))
                    })
            })
            .count();
        (self.current_smooth_key(idx).is_some() || self.current_iso_key(idx).is_some())
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
    ///
    /// Laid out as a Dear ImGui property panel (`docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md` §13.9):
    /// the mode as one segmented row with a status line under it, then folding sections —
    /// Camera, Volume, Opacity curve, Isosurface, Overlays, Slice — of label-left, control-right
    /// rows sized to the column, so nothing pushes it into scrolling sideways.
    pub(crate) fn map_3d_controls_body(&mut self, idx: usize, ui: &mut egui::Ui) {
        // The workstation's windows already run under `ws::style_scope`; the floating "3D map"
        // window of the other layouts keeps its theme, and the same components take its colours.
        let accent = crate::theme::accent(self.settings.theme);
        let t = if self.workstation_chrome() {
            ws::Tokens::new(accent)
        } else {
            ws::Tokens::from_visuals(ui.visuals(), accent)
        };
        let volume_supported = self.volume3d_supported;
        let smooth_current = self.current_smooth_key(idx);
        let iso_current = self.current_iso_key(idx);
        let smooth_ready = smooth_current
            .as_ref()
            .is_some_and(|key| self.smooth_vol_key[idx].as_ref() == Some(key));
        let smooth_info = self.smooth_vol_info[idx].filter(|_| smooth_ready);
        let product_range = self.smooth_vol_range[idx].filter(|_| smooth_ready);
        let failed: Vec<JobKey> = smooth_current
            .clone()
            .map(|key| JobKey::Smooth(idx, key))
            .into_iter()
            .chain(iso_current.clone().map(|key| JobKey::Iso(idx, key)))
            .filter(|key| self.loop3d_jobs.came_up_empty(key))
            .collect();
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
        let behind = (smooth_current.is_some() && !smooth_ready)
            || iso_current.as_ref().is_some_and(|key| {
                self.iso_mesh[idx]
                    .as_ref()
                    .is_none_or(|(accepted, _)| accepted != key)
            });
        let inputs = Panel3dInputs {
            volume_supported,
            smooth_info,
            product_range,
            failed: !failed.is_empty(),
            behind,
            loop_progress,
            products,
            cappi_alt_km: self.cappi_alt_km,
            selected_storm: self.selected_storm().map(|c| c.id),
        };
        let out = map_3d_panel(
            ui,
            &t,
            idx,
            &inputs,
            &mut self.views[idx],
            &mut self.settings,
        );
        if out.retry {
            for key in &failed {
                self.loop3d_jobs.retry(key);
            }
            // An accepted empty result must also be released before retrying.
            self.smooth_vol_key[idx] = None;
            ui.ctx().request_repaint();
        }
        if out.presets_changed {
            self.settings.save();
        }
        if let Some(roi) = out.roi {
            self.set_volume_roi(idx, roi);
        }
    }
}

/// What the 3D panel shows that comes from the app's 3D builds rather than from the pane.
pub(crate) struct Panel3dInputs {
    pub volume_supported: bool,
    /// The accepted smooth volume's cell size (km) and the fraction of echo outside its box.
    pub smooth_info: Option<(f32, f32)>,
    /// The accepted user-product volume's value range.
    pub product_range: Option<(f32, f32)>,
    /// A selected 3D build came up empty.
    pub failed: bool,
    /// A selected 3D build has not caught up with the time shown.
    pub behind: bool,
    pub loop_progress: Option<(usize, usize)>,
    /// User products by name, and whether each has a value at a single gate.
    pub products: Vec<(String, bool)>,
    pub cappi_alt_km: f32,
    /// The selected storm's ID, when one is selected (a region of interest goes around it).
    pub selected_storm: Option<String>,
}

/// What the panel asks of the app after a frame.
#[derive(Debug, Default)]
pub(crate) struct Panel3dOutcome {
    pub retry: bool,
    pub presets_changed: bool,
    /// Build around the selected storm at this half-width (`Some(Some(km))`), or go back to the
    /// whole radar (`Some(None)`).
    pub roi: Option<Option<f32>>,
}

/// The 3D panel for pane `idx` over its own view and the settings it shares (display units, the
/// sweep policy, volume presets). A free function so a test can draw the real panel.
pub(crate) fn map_3d_panel(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    idx: usize,
    inputs: &Panel3dInputs,
    view: &mut MapView,
    settings: &mut Settings,
) -> Panel3dOutcome {
    let t = *t;
    let Panel3dInputs {
        volume_supported,
        smooth_info,
        product_range,
        failed,
        behind,
        loop_progress,
        ref products,
        cappi_alt_km,
        ref selected_storm,
    } = *inputs;
    let mut out = Panel3dOutcome::default();
    let moment = view.moment;
    let units = display_units(moment, settings);
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

    // Mode. Of the eight representations at most three can be chosen at once (Observed, the
    // volume of the pane's own moment, and a user product on reflectivity), so three
    // segments rather than eight chips that wrapped a docked column into single letters.
    let volume_rep = volume_representation(moment);
    let user_ok = volume_supported && moment == Moment::Reflectivity && !products.is_empty();
    let segments = [
        ws::Segment {
            label: "Observed",
            enabled: true,
            hover: "Every real Level II tilt; no synthetic sweeps",
        },
        ws::Segment {
            label: "Volume",
            enabled: volume_supported && volume_rep.is_some(),
            hover: if volume_supported {
                volume_hover(moment)
            } else {
                "This device's graphics cannot hold the 3D texture a resampled volume needs"
            },
        },
        ws::Segment {
            label: "User",
            enabled: user_ok,
            hover: "A user-defined product (Tools > User products) worked out at every gate \
                    of the scan and drawn as a volume, like any moment. Shown while the map \
                    is on reflectivity.",
        },
    ];
    let selected = match view.map_3d.representation {
        Map3dRepresentation::ObservedSweeps => 0,
        Map3dRepresentation::SmoothProduct => 2,
        _ => 1,
    };
    match ws::segmented_full(ui, &t, &segments, Some(selected), true) {
        Some(0) => view.map_3d.representation = Map3dRepresentation::ObservedSweeps,
        Some(1) => {
            if let Some(rep) = volume_rep {
                view.map_3d.representation = rep;
            }
        }
        Some(2) => view.map_3d.representation = Map3dRepresentation::SmoothProduct,
        _ => {}
    }
    let rep = view.map_3d.representation;
    let observed = rep == Map3dRepresentation::ObservedSweeps;
    // Observed sweeps are ready once the layers built for this pane's own source are in.
    let observed_ready = view.map_3d.observed_key.as_ref().is_some_and(|key| {
        key.matches_source(view, moment, super::radar_products::policy(view, settings))
    }) && !view.map_3d.observed_layers.is_empty();
    // KDP is drawn on the map plane in Observed mode, so it never waits for layers.
    let observed_waiting =
        observed && !observed_ready && moment != Moment::SpecificDifferentialPhase;

    // Status: what is drawn, and whether it matches the time shown.
    ui.horizontal(|ui| {
        let (dot, state) = if failed {
            (t.danger, "no result")
        } else if behind {
            (t.warn, "building")
        } else if observed_waiting {
            (t.warn, "waiting")
        } else {
            (t.live, "current")
        };
        ws::status_dot(ui, dot, 3.5);
        ui.label(ws::text(rep.label(), 12.0, t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(ws::mono(state, 11.0, t.text_faint));
        });
    });
    if failed {
        ws::note(ui, &t, "No renderable 3D result for the selected inputs");
        out.retry = ws::button(ui, &t, "Retry 3D build", 0.0).clicked();
    } else if behind && !observed {
        ws::note(
            ui,
            &t,
            "Building the selected 3D frame; unmatched content is hidden",
        );
    }
    if let Some((built, total)) = loop_progress.filter(|(b, t)| b < t) {
        ws::note(ui, &t, format!("Loop 3D: {built} of {total} frames built")).on_hover_text(
            "Playback waits briefly for each frame's 3D, so the volume always matches \
             the time shown. Loop frames use a smaller grid; pause to see \
             the frame at full resolution.",
        );
    }
    ui.add_space(2.0);

    let camera = format!("{:.0}° · {:+.0}°", view.camera.pitch, view.camera.bearing);
    ws::fold_section(
        ui,
        &t,
        ("map3d_camera", idx),
        "Camera",
        Some(&camera),
        true,
        |ui| {
            ws::prop_slider(
                ui,
                &t,
                "Pitch",
                egui::Slider::new(
                    &mut view.camera.pitch,
                    0.0..=crate::render::mercator::MAX_PITCH_DEG,
                )
                .suffix("°")
                .max_decimals(0),
            );
            ws::prop_row(ui, &t, "Bearing", |ui| {
                let gap = ui.spacing().item_spacing.x;
                ui.spacing_mut().slider_width =
                    ws::prop_slider_width(ui, ws::CONTROL_H + 2.0 * gap);
                ws::slider(
                    ui,
                    &t,
                    egui::Slider::new(&mut view.camera.bearing, -180.0..=180.0)
                        .suffix("°")
                        .max_decimals(0),
                );
                let off_north = view.camera.bearing.abs() > 0.1;
                if ui
                    .add_enabled_ui(off_north, |ui| {
                        ws::glyph_button(ui, &t, ph::COMPASS, "Face north")
                    })
                    .inner
                    .on_hover_text("Face north")
                    .clicked()
                {
                    view.camera.bearing = 0.0;
                }
            });
            ws::prop_slider(
                ui,
                &t,
                "Vertical",
                egui::Slider::new(&mut view.map_3d.vertical_exaggeration, 1.0..=8.0)
                    .suffix("×")
                    .fixed_decimals(1),
            )
            .on_hover_text("Vertical exaggeration: heights are drawn this many times taller");
        },
    );

    let volume_summary = if observed {
        format!("{} tilts", view.map_3d.observed_layers.len())
    } else {
        crate::view::quality_label(view.map_3d.quality_steps)
            .unwrap_or("custom")
            .to_lowercase()
    };
    ws::fold_section(
        ui,
        &t,
        ("map3d_volume", idx),
        "Volume",
        Some(&volume_summary),
        true,
        |ui| {
            ws::prop_slider(
                ui,
                &t,
                "Opacity",
                egui::Slider::new(&mut view.map_3d.opacity, 0.1..=1.0).fixed_decimals(2),
            );
            if observed {
                ws::prop_slider(
                    ui,
                    &t,
                    "Beam rise",
                    egui::Slider::new(&mut view.map_3d.beam_rise, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .custom_parser(|s| {
                            s.trim_end_matches('%')
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|x| x / 100.0)
                        }),
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
                    cc_anomaly_rows(ui, &t, &mut view.map_3d.cc_anomaly);
                } else {
                    // Same `threshold_enabled`/`thresholds` the 2D "Product settings"
                    // Threshold control edits — one gate, so turning it on here also
                    // denoises the flat 2D view and vice versa, rather than a second
                    // floor a user has to keep in sync with the first.
                    let mi = moment.index();
                    let (vmin, vmax) = moment.value_range();
                    let (unit_factor, unit_label) = units;
                    let f = unit_factor as f64;
                    let mut on = view.threshold_enabled[mi];
                    // Shown greyed at the midpoint until first used, without writing it.
                    let mut value = view.thresholds[mi].unwrap_or((vmin + vmax) * 0.5);
                    let mut moved = false;
                    ws::prop_toggle(ui, &t, &mut on, "Denoise", |ui| {
                        ui.spacing_mut().slider_width = ws::prop_slider_width(ui, 0.0);
                        moved = ui
                            .add(
                                egui::Slider::new(&mut value, vmin..=vmax)
                                    .custom_formatter(move |v, _| format!("{:.0}", v * f))
                                    .custom_parser(move |s| s.parse::<f64>().ok().map(|x| x / f))
                                    .suffix(format!(" {unit_label}")),
                            )
                            .changed();
                    })
                    .on_hover_text(
                        "Hide everything below a value, so light rain and noise don't \
                         clutter the 3D scan",
                    );
                    view.threshold_enabled[mi] = on;
                    if on && (moved || view.thresholds[mi].is_none()) {
                        view.thresholds[mi] = Some(value);
                    }
                }
                if moment == Moment::SpecificDifferentialPhase {
                    ws::note(ui, &t, "KDP is derived; shown on the map plane.");
                }
                if observed_ready {
                    let layers = view.map_3d.observed_layers.clone();
                    ui::volume3d_window::layers_section(
                        ui,
                        ("map_3d_layers", idx),
                        &layers,
                        &mut view.map_3d.selected_layer_elevs,
                        moment.units(),
                        "Click a tilt to pull it out and see its stats — click more to \
                         compare several at once. Stats use the accepted sweep policy; \
                         excluded cuts can be empty. Inspect source coverage in the \
                         Inspector.",
                    );
                } else if moment != Moment::SpecificDifferentialPhase {
                    ws::note(ui, &t, "Waiting for matching observed layers");
                }
                return;
            }
            if rep == Map3dRepresentation::SmoothProduct {
                if view.map_3d.product.is_none() {
                    view.map_3d.product = products.iter().find(|p| p.1).map(|p| p.0.clone());
                }
                let shown = view
                    .map_3d
                    .product
                    .clone()
                    .unwrap_or_else(|| "(none)".into());
                ws::prop_row(ui, &t, "Product", |ui| {
                    egui::ComboBox::from_id_salt(("map3d_product", idx))
                        .selected_text(shown)
                        .width(ui.available_width() - 2.0)
                        .show_ui(ui, |ui| {
                            for (name, ok) in products {
                                ui.add_enabled_ui(*ok, |ui| {
                                    ui.selectable_value(
                                        &mut view.map_3d.product,
                                        Some(name.clone()),
                                        name,
                                    )
                                })
                                .response
                                .on_disabled_hover_text(
                                    "A vertical/layer function has no value at a single \
                                     gate, so this product has no volume",
                                );
                            }
                        });
                });
                ws::note(
                    ui,
                    &t,
                    match product_range {
                        Some((lo, hi)) => {
                            format!("Drawn from {lo:.2} (blue) to {hi:.2} (magenta)")
                        }
                        None => "Building the product volume".to_string(),
                    },
                );
            }
            // `SmoothDebris`'s inverted-CC volume has no floor of its own here — low CC
            // is the interesting case there, not high — so it gets the CC anomaly ramp
            // instead, and every other volume a Denoise floor against its own field, so
            // switching between them never carries one moment's number into another's
            // units.
            if let Some(((lo, hi), suffix)) = floor_range(rep, product_range) {
                let rep_i = rep as usize;
                let mut floor = floor_value(&mut view.map_3d, rep).clamp(lo, hi);
                let mut on = view.map_3d.denoise_enabled;
                ws::prop_toggle(ui, &t, &mut on, "Denoise", |ui| {
                    ui.spacing_mut().slider_width = ws::prop_slider_width(ui, 0.0);
                    ws::slider(
                        ui,
                        &t,
                        egui::Slider::new(&mut floor, lo..=hi)
                            .suffix(suffix)
                            .max_decimals(floor_decimals(suffix)),
                    );
                })
                .on_hover_text(
                    "Hide weak values below the floor so the interesting structure \
                     stands alone",
                );
                // The value window's other end: with a ceiling too, only one band of
                // values stays, such as the 45-55 dBZ shell around a hail core.
                let ceiling = &mut view.map_3d.ceilings[rep_i];
                let mut capped = ceiling.is_some();
                let mut top = ceiling.unwrap_or(hi.min(floor + (hi - lo) * 0.2));
                ui.add_enabled_ui(on, |ui| {
                    ws::prop_toggle(ui, &t, &mut capped, "Ceiling", |ui| {
                        ui.spacing_mut().slider_width = ws::prop_slider_width(ui, 0.0);
                        ws::slider(
                            ui,
                            &t,
                            egui::Slider::new(&mut top, floor..=hi)
                                .suffix(suffix)
                                .max_decimals(floor_decimals(suffix)),
                        );
                    })
                    .on_hover_text(
                        "Also hide values above this, keeping one band between the floor \
                         and the ceiling",
                    );
                });
                *ceiling = capped.then_some(top);
                *floor_value(&mut view.map_3d, rep) = floor;
                view.map_3d.denoise_enabled = on;
            }
            if rep == Map3dRepresentation::SmoothDebris {
                cc_anomaly_rows(ui, &t, &mut view.map_3d.cc_anomaly);
            }
            // Region of interest (M3.6): the same voxel budget spent around one storm.
            ws::prop_row(ui, &t, "Region", |ui| {
                let label = match &view.map_3d.roi {
                    None => "Whole radar".to_string(),
                    Some(r) => format!(
                        "{:.0} km around {}",
                        r.half_km * 2.0,
                        r.storm
                            .as_deref()
                            .map_or("a point".into(), |s| format!("storm {s}"))
                    ),
                };
                egui::ComboBox::from_id_salt(("map3d_roi", idx))
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(view.map_3d.roi.is_none(), "Whole radar")
                            .clicked()
                        {
                            out.roi = Some(None);
                        }
                        for km in crate::view::ROI_SIZES_KM {
                            let on = view.map_3d.roi.as_ref().is_some_and(|r| r.half_km == km);
                            let resp = ui.add_enabled(
                                selected_storm.is_some(),
                                egui::Button::selectable(
                                    on,
                                    format!("{:.0} km around the selected storm", km * 2.0),
                                ),
                            );
                            if resp
                                .on_disabled_hover_text("Select a storm first")
                                .clicked()
                            {
                                out.roi = Some(Some(km));
                            }
                        }
                    });
            })
            .response
            .on_hover_text(
                "Build the smooth volume around one storm instead of the whole radar: the same \
                 number of voxels over a smaller box gives finer cells",
            );
            if let Some(roi) = view.map_3d.roi.as_mut() {
                if roi.lost {
                    ws::note(
                        ui,
                        &t,
                        "The storm left the storm table: the region stays where it last was"
                            .to_string(),
                    );
                } else if roi.storm.is_some() {
                    ws::check(ui, &t, &mut roi.follow, "Follow the storm").on_hover_text(
                        "Move the region with the storm as new scans arrive; it stops if the \
                         storm is lost or another is selected",
                    );
                }
            }
            ws::prop_row(ui, &t, "Render", |ui| {
                let modes = crate::render3d::VolumeRender::ALL;
                let segments: Vec<ws::Segment<'_>> =
                    modes.iter().map(|m| ws::Segment::new(m.label())).collect();
                let at = modes.iter().position(|m| *m == view.map_3d.volume_render);
                if let Some(i) = ws::segmented_full(ui, &t, &segments, at, true) {
                    view.map_3d.volume_render = modes[i];
                }
            })
            .response
            .on_hover_text(view.map_3d.volume_render.describe());
            ws::prop_row(ui, &t, "Quality", |ui| {
                let labels: Vec<&str> = crate::view::QUALITY_PRESETS
                    .iter()
                    .map(|(l, _)| *l)
                    .collect();
                let segments: Vec<ws::Segment<'_>> =
                    labels.iter().map(|l| ws::Segment::new(l)).collect();
                let at = crate::view::QUALITY_PRESETS
                    .iter()
                    .position(|(_, s)| *s == view.map_3d.quality_steps);
                if let Some(i) = ws::segmented_full(ui, &t, &segments, at, true) {
                    view.map_3d.quality_steps = crate::view::QUALITY_PRESETS[i].1;
                }
            });
            ws::check(ui, &t, &mut view.map_3d.smooth_full_range, "Full range").on_hover_text(
                "Off: the volume is cropped to the range holding 99% of the echo, \
                     which keeps its cells small. On: everything the radar reported, with \
                     coarser cells.",
            );
            if let Some((cell_km, outside)) = smooth_info {
                let mut line = format!("{cell_km:.2} km cells");
                if outside > 0.0005 {
                    line.push_str(&format!(
                        " · {:.1}% of echo outside the box",
                        outside * 100.0
                    ));
                }
                ws::note(ui, &t, line);
            }
            if rep == Map3dRepresentation::SmoothVelocity {
                ws::note(
                    ui,
                    &t,
                    if view.srv {
                        format!(
                            "Storm-relative: {:.0} kt from {:.0}\u{b0} taken off (SRV)",
                            view.storm_speed_kt,
                            (view.storm_dir_deg + 180.0).rem_euclid(360.0)
                        )
                    } else {
                        "Ground-relative (turn on SRV for storm-relative)".to_string()
                    },
                );
            }
        },
    );

    if let Some(((lo, hi), suffix)) = floor_range(rep, product_range) {
        let rep_i = rep as usize;
        let curve_on = if view.map_3d.tf_curves[rep_i].is_some() {
            "on"
        } else {
            "off"
        };
        ws::fold_section(
            ui,
            &t,
            ("map3d_curve", idx),
            "Opacity curve",
            Some(curve_on),
            false,
            |ui| {
                ui::volume3d_window::opacity_curve(
                    ui,
                    &mut view.map_3d.tf_curves[rep_i],
                    (lo, hi),
                    suffix,
                );
                // Only where the volume's colour table is linear in value (`lut_range`).
                let colors_available = rep
                    .smooth_moment()
                    .is_some_and(|(m, invert)| m != Moment::Velocity && !invert);
                ui::volume3d_window::color_stops(
                    ui,
                    &mut view.map_3d.color_stops[rep_i],
                    (lo, hi),
                    suffix,
                    colors_available,
                );
                let mut floor = floor_value(&mut view.map_3d, rep).clamp(lo, hi);
                out.presets_changed = ui::volume3d_window::presets_row(
                    ui,
                    &mut settings.volume3d_presets,
                    rep.label(),
                    &mut floor,
                    &mut view.map_3d.denoise_enabled,
                    &mut view.map_3d.ceilings[rep_i],
                    &mut view.map_3d.tf_curves[rep_i],
                    &mut view.map_3d.volume_render,
                    &mut view.map_3d.color_stops[rep_i],
                    &mut view.map_3d.preset_name,
                );
                *floor_value(&mut view.map_3d, rep) = floor;
            },
        );
    }

    // Isosurface (Phase H3): the pane's moment at one threshold, per-moment.
    let mi = Moment::ALL.iter().position(|m| *m == moment).unwrap_or(0);
    let (lo, hi) = moment.value_range();
    let lo = if moment == Moment::Velocity { 0.0 } else { lo };
    // Stored in native units whatever the display units are (velocity in m/s).
    let iso_unit = moment.units();
    let iso_summary = if view.map_3d.iso_enabled {
        let sign = if moment == Moment::Velocity { "±" } else { "" };
        let v = view.map_3d.iso_values[mi];
        let digits = if (v - v.round()).abs() < 0.005 { 0 } else { 2 };
        format!("{sign}{v:.digits$} {iso_unit}")
            .trim_end()
            .to_string()
    } else {
        "off".to_string()
    };
    ws::fold_section(
        ui,
        &t,
        ("map3d_iso", idx),
        "Isosurface",
        Some(&iso_summary),
        true,
        |ui| {
            ws::check(ui, &t, &mut view.map_3d.iso_enabled, "Draw isosurface").on_hover_text(
                "A surface where this product crosses a threshold: a 50 dBZ core, a 3 dB \
                     ZDR column, a low-CC debris pocket (low CC is inside). Velocity draws a \
                     pair: outbound at +threshold and inbound at -threshold, each in its \
                     palette colour.",
            );
            ui.add_enabled_ui(view.map_3d.iso_enabled, |ui| {
                let suffix = if iso_unit.is_empty() {
                    String::new()
                } else {
                    format!(" {iso_unit}")
                };
                ws::prop_slider(
                    ui,
                    &t,
                    if moment == Moment::Velocity {
                        "± Threshold"
                    } else {
                        "Threshold"
                    },
                    egui::Slider::new(&mut view.map_3d.iso_values[mi], lo..=hi)
                        .suffix(suffix)
                        .max_decimals(2),
                );
                let span = (hi - lo).abs();
                ws::prop_toggle(ui, &t, &mut view.map_3d.iso_nested, "Nested", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut view.map_3d.iso_steps[mi])
                            .range(span / 100.0..=span / 3.0)
                            .speed(span / 500.0)
                            .max_decimals(2)
                            .prefix("step "),
                    );
                })
                .on_hover_text(
                    "Two more surfaces inside the first, one and two steps further in, the \
                     outer ones fainter: a 40/50/60 dBZ envelope around its core",
                );
                ws::prop_slider(
                    ui,
                    &t,
                    "Opacity",
                    egui::Slider::new(&mut view.map_3d.iso_opacity, 0.1..=1.0).fixed_decimals(2),
                )
                .on_hover_text("How solid the surface is drawn");
                ws::prop_row(ui, &t, "Shading", |ui| {
                    ws::check(ui, &t, &mut view.map_3d.iso_lit, "Lit")
                        .on_hover_text("Shade by a light from the northwest so the shape reads");
                    ws::check(ui, &t, &mut view.map_3d.iso_smooth, "Smooth").on_hover_text(
                        "Display smoothing of the surface only; the radar values are never \
                         smoothed",
                    );
                });
            });
        },
    );

    let m3 = &mut view.map_3d;
    let shown = [
        m3.mrms_surface,
        m3.cloud_top_surface,
        m3.model_isotherms,
        m3.cell_columns,
        m3.height_ruler,
        m3.terrain,
        m3.beam_guides,
    ]
    .iter()
    .filter(|on| **on)
    .count();
    let overlays = format!("{shown} on");
    ws::fold_section(
        ui,
        &t,
        ("map3d_overlays", idx),
        "Overlays",
        Some(&overlays),
        true,
        |ui| {
            ui.columns(2, |cols| {
                // Analyses on the left, geometry on the right; the short labels keep each
                // inside half a column, the full description is on hover.
                let [left, right] = cols else {
                    return;
                };
                ws::check(left, &t, &mut m3.mrms_surface, "MRMS tops").on_hover_text(
                    "MRMS echo tops as a surface: a displayed MRMS echo-top layer \
                     (18/30/50/60 dBZ) at its height, translucent with an analysis grid over \
                     it, so the analysed MRMS surface never reads as the observed radar",
                );
                ws::check(left, &t, &mut m3.cloud_top_surface, "Cloud tops").on_hover_text(
                    "Satellite cloud tops as a surface: GOES cloud top height (NOAA's ACHA \
                     product, 10 km, every 5 minutes) at its height, pale grey to white, \
                     higher is whiter, fainter than the MRMS surface and without its grid",
                );
                ws::check(left, &t, &mut m3.model_isotherms, "Isotherms").on_hover_text(
                    "HRRR 0/-10/-20 °C surfaces: the HRRR's latest analysis of the 0, -10 and \
                     -20 °C heights (the hail-growth zone) as model surfaces, one colour per \
                     level with a dashed grid, the forecast look",
                );
                ws::check(right, &t, &mut m3.cell_columns, "Storm cells").on_hover_text(
                    "Storm cells as columns: each SCIT storm cell from base to top, a dot at \
                     the height of its strongest echo, TVS red and meso yellow, with its past \
                     track on the ground (needs storm cells on)",
                );
                ws::check(right, &t, &mut m3.height_ruler, "Height ruler").on_hover_text(
                    "A km MSL ruler standing at the view's centre, labelled every 4 km",
                );
                ws::check(right, &t, &mut m3.terrain, "Terrain").on_hover_text(
                    "The ground as a shaded surface at its height (AWS terrain tiles), at \
                     the same vertical exaggeration as everything else",
                );
                ws::check(right, &t, &mut m3.beam_guides, "Beam guides").on_hover_text(
                    "Draw the radar's beam geometry: each tilt's cone as rings at 50-200 km \
                     (low tilts cyan, high magenta), the lowest and highest beams toward the \
                     view with their 0.95° beamwidth edges, and the antenna mast",
                );
            });
        },
    );

    if !observed {
        let whole = view.map_3d.clip == [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let cut = !whole || view.map_3d.plane.is_some() || view.map_3d.cappi_marker;
        ws::fold_section(
            ui,
            &t,
            ("map3d_slice", idx),
            "Slice",
            Some(if cut { "on" } else { "off" }),
            false,
            |ui| {
                let [x0, x1, y0, y1, z0, z1] = &mut view.map_3d.clip;
                slice_row(ui, &t, "East–west", x0, x1);
                slice_row(ui, &t, "North–south", y0, y1);
                slice_row(ui, &t, "Height", z0, z1);
                ui.add_enabled_ui(!whole, |ui| {
                    if ws::button(ui, &t, "Whole volume", 0.0).clicked() {
                        view.map_3d.clip = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
                    }
                });
                ui.add_space(2.0);
                ui::volume3d_window::plane_controls(ui, &mut view.map_3d.plane);
                ui::volume3d_window::cappi_marker_controls(
                    ui,
                    &mut view.map_3d.cappi_marker,
                    cappi_alt_km,
                );
            },
        );
    }
    ui.add_space(4.0);
    ws::note(
        ui,
        &t,
        "Right-drag rotates · drag pans · wheel zooms · W/S tilt · Q/E rotate",
    );
    out
}

/// The resampled volume of `moment`, the one representation the Volume segment chooses while
/// the pane shows it; `None` for ΦDP, which has no volume.
fn volume_representation(moment: Moment) -> Option<Map3dRepresentation> {
    Some(match moment {
        Moment::Reflectivity => Map3dRepresentation::SmoothVolume,
        Moment::CorrelationCoefficient => Map3dRepresentation::SmoothDebris,
        Moment::SpectrumWidth => Map3dRepresentation::SmoothSpectrumWidth,
        Moment::DifferentialReflectivity => Map3dRepresentation::SmoothZdr,
        Moment::SpecificDifferentialPhase => Map3dRepresentation::SmoothKdp,
        Moment::Velocity => Map3dRepresentation::SmoothVelocity,
        Moment::DifferentialPhase => return None,
    })
}

/// What the Volume segment draws for `moment`.
fn volume_hover(moment: Moment) -> &'static str {
    match moment {
        Moment::Reflectivity => "Regularized reflectivity volume",
        Moment::CorrelationCoefficient => {
            "Lofted low correlation coefficient — possible tornado debris (TDS). Brighter = \
             lower CC, inverted from the usual CC scale."
        }
        Moment::SpectrumWidth => {
            "Regularized spectrum-width volume — shear and turbulence signatures, the same \
             continuous fill as reflectivity and debris"
        }
        Moment::DifferentialReflectivity => {
            "Regularized ZDR volume — ZDR columns above the melting level mark strong updrafts"
        }
        Moment::SpecificDifferentialPhase => {
            "Regularized KDP volume — heavy rain and melting-hail cores"
        }
        Moment::Velocity => {
            "Dealiased velocity volume, strongest wind along each line of sight in either \
             direction: both halves of a couplet show, inbound and outbound in their own \
             colours. Where strong inbound meets strong outbound the boundary takes one colour \
             or the other."
        }
        Moment::DifferentialPhase => {
            "ΦDP has no volume: switch the map to REF, VEL, SW, ZDR, KDP or CC for one"
        }
    }
}

/// The Denoise floor's range and unit for a resampled representation, or `None` for the two
/// without a floor: Observed (which uses the pane's 2D threshold) and Debris (the CC anomaly
/// ramp). A user product's range is its built volume's, so it has none until that is built.
fn floor_range(
    rep: Map3dRepresentation,
    product_range: Option<(f32, f32)>,
) -> Option<((f32, f32), &'static str)> {
    Some(match rep {
        Map3dRepresentation::SmoothVolume => (Moment::Reflectivity.value_range(), " dBZ"),
        Map3dRepresentation::SmoothSpectrumWidth => (Moment::SpectrumWidth.value_range(), " m/s"),
        Map3dRepresentation::SmoothZdr => (Moment::DifferentialReflectivity.value_range(), " dB"),
        Map3dRepresentation::SmoothKdp => {
            (Moment::SpecificDifferentialPhase.value_range(), " °/km")
        }
        // A speed, either direction.
        Map3dRepresentation::SmoothVelocity => ((0.0, Moment::Velocity.value_range().1), " m/s"),
        Map3dRepresentation::SmoothProduct => (product_range?, ""),
        Map3dRepresentation::SmoothDebris | Map3dRepresentation::ObservedSweeps => return None,
    })
}

/// Decimals a floor shows: whole dBZ and m/s are as fine as anyone sets a floor, and keep the
/// value inside its box; dB and °/km span a few units, so tenths.
fn floor_decimals(suffix: &str) -> usize {
    match suffix {
        " dBZ" | " m/s" => 0,
        "" => 2,
        _ => 1,
    }
}

/// The floor field `rep` keeps, each against its own so switching representations never
/// carries one moment's number into another's units. Debris and Observed have none and share
/// the reflectivity field only to keep this total; [`floor_range`] never offers them a row.
fn floor_value(m: &mut crate::view::Map3dState, rep: Map3dRepresentation) -> &mut f32 {
    match rep {
        Map3dRepresentation::SmoothSpectrumWidth => &mut m.sw_floor_ms,
        Map3dRepresentation::SmoothZdr => &mut m.zdr_floor_db,
        Map3dRepresentation::SmoothKdp => &mut m.kdp_floor_deg_km,
        Map3dRepresentation::SmoothVelocity => &mut m.velocity_floor_ms,
        Map3dRepresentation::SmoothProduct => &mut m.product_floor,
        Map3dRepresentation::SmoothVolume
        | Map3dRepresentation::SmoothDebris
        | Map3dRepresentation::ObservedSweeps => &mut m.reflectivity_floor_dbz,
    }
}

/// One `min..max` pair of sliders for an axis of the map-embedded 3D volume's slab, as a
/// property row: the two tracks share the control column, the low end first.
fn slice_row(ui: &mut egui::Ui, t: &ws::Tokens, label: &str, lo: &mut f32, hi: &mut f32) {
    ws::prop_row(ui, t, label, |ui| {
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().slider_width = ((ui.available_width() - gap) / 2.0).max(30.0);
        crate::theme::slider(ui, egui::Slider::new(lo, 0.0..=1.0).show_value(false))
            .on_hover_text("Low end of the slab");
        crate::theme::slider(ui, egui::Slider::new(hi, 0.0..=1.0).show_value(false))
            .on_hover_text("High end of the slab");
    });
    // Keep the pair ordered so an inverted drag empties the view instead of inverting the slab.
    if *lo > *hi {
        std::mem::swap(lo, hi);
    }
}

/// The CC-anomaly rows, shared by the Observed (correlation coefficient) and Debris 3D modes so
/// the two cannot drift into describing the same thing differently.
///
/// The thresholds are all user-set on purpose. The defaults are a reasonable starting point for
/// a warm-season CONUS debris hunt and nothing more — CC backgrounds move with the radar, the
/// beam's distance, the precipitation type and the season, so anything presented here as a fixed
/// meteorological constant would be wrong somewhere.
fn cc_anomaly_rows(ui: &mut egui::Ui, t: &ws::Tokens, a: &mut crate::view::CcAnomaly) {
    let mut on = a.enabled;
    let mut reset = false;
    ws::prop_toggle(ui, t, &mut on, "Anomaly", |ui| {
        reset = ws::glyph_button(ui, t, ph::ARROW_COUNTER_CLOCKWISE, "Reset")
            .on_hover_text("Back to 0.97 / 0.80")
            .clicked();
    })
    .on_hover_text(
        "Fade ordinary high-CC precipitation toward transparent and make low CC progressively \
         more solid, so lofted debris, clutter and other non-meteorological returns stand out \
         of the storm around them. The opposite of a denoise floor, which for CC would hide \
         exactly those.",
    );
    a.enabled = on;
    if reset {
        *a = crate::view::CcAnomaly::default();
    }
    ui.add_enabled_ui(a.enabled, |ui| {
        ws::prop_slider(
            ui,
            t,
            "Clear above",
            egui::Slider::new(&mut a.clear_cc, 0.80..=1.0)
                .custom_formatter(|v, _| format!("{v:.3}")),
        )
        .on_hover_text("CC at or above this draws faintest — the background scatter edge");
        ws::prop_slider(
            ui,
            t,
            "Solid below",
            egui::Slider::new(&mut a.opaque_cc, 0.0..=0.99)
                .custom_formatter(|v, _| format!("{v:.3}")),
        )
        .on_hover_text("CC at or below this draws at full strength");
        ws::prop_slider(
            ui,
            t,
            "Faintest",
            egui::Slider::new(&mut a.faintest, 0.0..=0.5)
                .custom_formatter(|v, _| format!("{v:.2}")),
        )
        .on_hover_text(
            "How visible the background stays. Zero removes it entirely; a little left keeps \
             the storm as context around the anomaly.",
        );
    });
    // Ordering here rather than clamping each slider against the other: mutually-constrained
    // sliders feel stuck, and `cc_anomaly_uniform` already orders the pair before it builds the
    // ramp, so a crossed pair is only ever a display question.
    if a.opaque_cc > a.clear_cc {
        std::mem::swap(&mut a.opaque_cc, &mut a.clear_cc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 3D panel at the widths it is docked at (a tablet's right dock, the floating window's
    /// minimum), for each kind of mode, written for review under `target/parity-review`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the 3D view panel captures for review"]
    fn gpu_map_3d_panel_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for 3D panel review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/map-3d-panel");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for (name, rep, moment, width) in [
            (
                "observed",
                Map3dRepresentation::ObservedSweeps,
                Moment::Reflectivity,
                300,
            ),
            (
                "volume",
                Map3dRepresentation::SmoothVolume,
                Moment::Reflectivity,
                300,
            ),
            (
                "volume-narrow",
                Map3dRepresentation::SmoothVolume,
                Moment::Reflectivity,
                240,
            ),
            (
                "debris",
                Map3dRepresentation::SmoothDebris,
                Moment::CorrelationCoefficient,
                300,
            ),
        ] {
            let mut view = MapView::new(Some("KTLX".into()), Camera::at_lonlat(-97.5, 35.3, 9.0));
            view.moment = moment;
            view.map_3d.enabled = true;
            view.map_3d.representation = rep;
            view.camera.pitch = 71.34;
            view.camera.bearing = -76.45;
            view.map_3d.iso_enabled = rep == Map3dRepresentation::SmoothVolume;
            view.map_3d.cell_columns = true;
            view.map_3d.height_ruler = true;
            view.map_3d.denoise_enabled = true;
            let mut settings = Settings::default();
            let inputs = Panel3dInputs {
                volume_supported: true,
                smooth_info: Some((0.21, 0.002)),
                product_range: None,
                failed: false,
                behind: name == "volume-narrow",
                loop_progress: None,
                products: Vec::new(),
                cappi_alt_km: 3.0,
                selected_storm: None,
            };
            gpu.save(
                &destination.join(format!("{name}-{width}.png")),
                width,
                1000,
                |ui| {
                    ws::style_scope(ui, &t);
                    egui::Frame::NONE
                        .fill(t.panel)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(width as f32 - 20.0);
                            map_3d_panel(ui, &t, 0, &inputs, &mut view, &mut settings);
                        });
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn every_volume_segment_choice_belongs_to_its_moment() {
        for m in Moment::ALL {
            if let Some(rep) = volume_representation(m) {
                assert_eq!(rep.smooth_moment().map(|(mm, _)| mm), Some(m), "{m:?}");
            }
        }
        assert_eq!(volume_representation(Moment::DifferentialPhase), None);
    }

    #[test]
    fn floors_are_offered_exactly_where_a_field_exists() {
        assert!(floor_range(Map3dRepresentation::ObservedSweeps, None).is_none());
        assert!(floor_range(Map3dRepresentation::SmoothDebris, None).is_none());
        assert!(floor_range(Map3dRepresentation::SmoothProduct, None).is_none());
        assert_eq!(
            floor_range(Map3dRepresentation::SmoothProduct, Some((1.0, 2.0))),
            Some(((1.0, 2.0), ""))
        );
        let mut m = crate::view::Map3dState::default();
        *floor_value(&mut m, Map3dRepresentation::SmoothZdr) = 2.5;
        assert_eq!(m.zdr_floor_db, 2.5);
        *floor_value(&mut m, Map3dRepresentation::SmoothVelocity) = 9.0;
        assert_eq!(m.velocity_floor_ms, 9.0);
    }
}
