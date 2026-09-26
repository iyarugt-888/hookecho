//! Settings that shape a layer you already turned on, plus the action struct the chrome raises.
//!
//! This is what survived the Advanced toolbox: the toolbox's Site / Map / Product / Timeline
//! sections were all fourth copies of controls the site dialog, the drawer, the product pill and
//! the timeline pill already own. Only the per-layer knobs had no other home, so they moved into
//! the drawer's "Layer options" section — and, like before, a knob only renders when its layer is
//! actually on, so the section is short (usually empty) instead of a wall of dead controls.

use crate::app::OverlayFilters;
use crate::render::FieldLayer;
use crate::ui::a11y::Named as _;
use wxdata::alerts::Category;

/// Signals the chrome (drawer, pills, mobile sheets) raises for the app to act on this frame.
#[derive(Default)]
pub struct UiActions {
    pub open_site_dialog: bool,
    pub reload: bool,
    /// An overlay filter toggle changed; the app should reassemble the displayed set.
    pub overlays_changed: bool,
    /// Set the active view's storm motion from the SCIT storm-cell mean motion.
    pub srv_from_cells: bool,
    /// DVR: replay the buffered (in-RAM) frames from the earliest cached one.
    pub instant_replay: bool,
    /// Rebuild the max/min trail from its cached window ending at the current playhead.
    pub reset_trail: bool,
    /// The Day-1 outlook hazard changed; the app must clear + refetch that day's outlook.
    pub outlook_kind_changed: bool,
    /// The WSSI day changed; the app must clear + refetch it.
    pub wssi_day_changed: bool,
    /// The Excessive Rainfall Outlook day changed; the app must clear + refetch it.
    pub ero_day_changed: bool,
    /// The Fire Weather Outlook day changed; the app must clear + refetch it.
    pub fire_day_changed: bool,
    /// Start an offline chase-pack download of the current view's basemap.
    pub download_chasepack: bool,
    /// Cancel the in-progress chase-pack download.
    pub cancel_chasepack: bool,
    /// A row in the embedded layers registry was clicked; the app applies it.
    pub(crate) palette: Option<crate::app::PaletteAction>,
    /// Pick one MRMS QPE accumulation window in the active pane, keeping existing layer slugs.
    pub(crate) qpe_window: Option<FieldLayer>,
    /// Pick one MRMS echo-top reflectivity threshold in the active pane.
    pub(crate) echo_top_threshold: Option<FieldLayer>,
    /// Pick one MRMS isothermal reflectivity level in the active pane.
    pub(crate) isotherm_level: Option<FieldLayer>,
    /// Pick one FLASH QPE average-recurrence-interval window in the active pane.
    pub(crate) flash_ari_window: Option<FieldLayer>,
}

/// Existing catalog-backed QPE layers stay distinct for saved-workspace and headless slug
/// compatibility. The picker treats them as one choice in the active pane.
pub(crate) const QPE_WINDOWS: [(FieldLayer, &str); 5] = [
    (FieldLayer::Qpe1h, "1 hour"),
    (FieldLayer::Qpe3h, "3 hours"),
    (FieldLayer::Qpe6h, "6 hours"),
    (FieldLayer::Qpe12h, "12 hours"),
    (FieldLayer::Qpe24h, "24 hours"),
];

/// These are distinct catalog products and stable workspace slugs, sharing one km MSL legend.
pub(crate) const ECHO_TOP_THRESHOLDS: [(FieldLayer, &str); 4] = [
    (FieldLayer::MrmsEchoTop18, "18 dBZ"),
    (FieldLayer::MrmsEchoTop30, "30 dBZ"),
    (FieldLayer::MrmsEchoTop50, "50 dBZ"),
    (FieldLayer::MrmsEchoTop60, "60 dBZ"),
];

pub(crate) const ISOTHERM_LEVELS: [(FieldLayer, &str); 5] = [
    (FieldLayer::MrmsRefl0c, "0°C"),
    (FieldLayer::MrmsReflM5c, "-5°C"),
    (FieldLayer::MrmsReflM10c, "-10°C"),
    (FieldLayer::MrmsReflM15c, "-15°C"),
    (FieldLayer::MrmsReflM20c, "-20°C"),
];

pub(crate) const FLASH_ARI_WINDOWS: [(FieldLayer, &str); 7] = [
    (FieldLayer::FlashFlood, "30m"),
    (FieldLayer::FlashFlood1h, "1h"),
    (FieldLayer::FlashFlood3h, "3h"),
    (FieldLayer::FlashFlood6h, "6h"),
    (FieldLayer::FlashFlood12h, "12h"),
    (FieldLayer::FlashFlood24h, "24h"),
    (FieldLayer::FlashFloodMax, "Max"),
];

fn rotation_window_label(minutes: u16) -> &'static str {
    match minutes {
        60 => "1h",
        120 => "2h",
        240 => "4h",
        360 => "6h",
        1440 => "24h",
        _ => "30m",
    }
}

fn select_one_of(
    on: &mut std::collections::HashSet<FieldLayer>,
    choices: &[(FieldLayer, &str)],
    selected: FieldLayer,
) -> bool {
    if !choices.iter().any(|(layer, _)| *layer == selected) {
        return false;
    }
    for (layer, _) in choices {
        on.remove(layer);
    }
    on.insert(selected);
    true
}

pub(crate) fn select_qpe_window(
    on: &mut std::collections::HashSet<FieldLayer>,
    selected: FieldLayer,
) -> bool {
    select_one_of(on, &QPE_WINDOWS, selected)
}

pub(crate) fn select_echo_top_threshold(
    on: &mut std::collections::HashSet<FieldLayer>,
    selected: FieldLayer,
) -> bool {
    select_one_of(on, &ECHO_TOP_THRESHOLDS, selected)
}

pub(crate) fn select_isotherm_level(
    on: &mut std::collections::HashSet<FieldLayer>,
    selected: FieldLayer,
) -> bool {
    select_one_of(on, &ISOTHERM_LEVELS, selected)
}

pub(crate) fn select_flash_ari_window(
    on: &mut std::collections::HashSet<FieldLayer>,
    selected: FieldLayer,
) -> bool {
    select_one_of(on, &FLASH_ARI_WINDOWS, selected)
}

fn catalog_choice_control(
    ui: &mut egui::Ui,
    on: &std::collections::HashSet<FieldLayer>,
    id: &'static str,
    label: &'static str,
    name: &'static str,
    hint: &'static str,
    choices: &[(FieldLayer, &str)],
) -> Option<FieldLayer> {
    let active: Vec<_> = choices
        .iter()
        .filter(|(layer, _)| on.contains(layer))
        .collect();
    let selected = match active.as_slice() {
        [] => "Choose",
        [(_, label)] => *label,
        _ => "Multiple",
    };
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for &(layer, label) in choices {
                    if ui.selectable_label(on.contains(&layer), label).clicked() {
                        pick = Some(layer);
                    }
                }
            })
            .response
            .named(name)
            .on_hover_text(hint);
    });
    pick
}

/// Shared compact control for the workstation and the other layer-options surfaces.
pub(crate) fn qpe_window_control(
    ui: &mut egui::Ui,
    on: &std::collections::HashSet<FieldLayer>,
    actions: &mut UiActions,
) {
    actions.qpe_window = catalog_choice_control(
        ui,
        on,
        "mrms_qpe_window",
        "Rain total (QPE)",
        "MRMS rain total accumulation window",
        "Choose one MRMS rain-accumulation window for this pane",
        &QPE_WINDOWS,
    );
}

pub(crate) fn echo_top_threshold_control(
    ui: &mut egui::Ui,
    on: &std::collections::HashSet<FieldLayer>,
    actions: &mut UiActions,
) {
    actions.echo_top_threshold = catalog_choice_control(
        ui,
        on,
        "mrms_echo_top_threshold",
        "Echo top (MRMS)",
        "MRMS echo-top reflectivity threshold",
        "Choose the 18, 30, 50 or 60 dBZ national echo-top height for this pane (km MSL)",
        &ECHO_TOP_THRESHOLDS,
    );
}

pub(crate) fn isotherm_level_control(
    ui: &mut egui::Ui,
    on: &std::collections::HashSet<FieldLayer>,
    actions: &mut UiActions,
) {
    actions.isotherm_level = catalog_choice_control(
        ui,
        on,
        "mrms_isotherm_level",
        "Refl. at temperature",
        "MRMS isothermal reflectivity level",
        "Choose the environmental temperature level for national isothermal reflectivity (dBZ)",
        &ISOTHERM_LEVELS,
    );
}

pub(crate) fn flash_ari_window_control(
    ui: &mut egui::Ui,
    on: &std::collections::HashSet<FieldLayer>,
    actions: &mut UiActions,
) {
    actions.flash_ari_window = catalog_choice_control(
        ui,
        on,
        "mrms_flash_ari_window",
        "Rainfall rarity (ARI)",
        "FLASH QPE average recurrence interval window",
        "Choose a QPE accumulation window or the maximum across windows; values are years, not flood probability",
        &FLASH_ARI_WINDOWS,
    );
}

/// Read-only chase-pack state the app feeds the UI each frame: the current-view estimate and,
/// while a download runs, its progress `(done, total, errors, mb)`.
pub struct ChasePackUi {
    pub tiles: u64,
    pub mb: f64,
    /// The active basemap can be pre-downloaded (raster with a URL, or vector once its template loads).
    pub packable: bool,
    pub z_lo: u8,
    pub z_hi: u8,
    pub progress: Option<(u64, u64, u64, f64)>,
}

/// Can STP be computed from this source? It needs an LCL height, which only the HRRR surface
/// file publishes — the RAP analysis and the NAM nest both leave it out.
pub(crate) fn stp_source(model: wxdata::hrrr::Model) -> bool {
    matches!(model, wxdata::hrrr::Model::Hrrr)
}

/// Both grids must have the same valid time; source runs and leads may differ.
fn valid_time_note(
    valid: Option<&crate::fielddiff::ComparisonTimes>,
    error: Option<&str>,
    a: &str,
    b: &str,
) -> String {
    match (valid, error) {
        (Some(times), _) => times.label(a, b),
        (None, Some(error)) => format!("⚠ Comparison unavailable: {error}"),
        (None, None) => {
            "Waiting for models at one shared valid time; no comparison is drawn.".into()
        }
    }
}

#[allow(clippy::too_many_arguments)] // one flat call per frame; a params struct adds churn for no reader gain
pub(crate) fn show(
    ui: &mut egui::Ui,
    filters: &mut OverlayFilters,
    fields: &mut std::collections::HashMap<crate::render::FieldLayer, crate::app::FieldState>,
    // Which layers the active pane draws. Visibility is per-pane now; the map above is the shared
    // fetch state, which is what `fields` is still needed for (clearing a refetch clock).
    on: &std::collections::HashSet<crate::render::FieldLayer>,
    analysis_time: Option<chrono::DateTime<chrono::Utc>>,
    time_tolerance: chrono::Duration,
    rotation_minutes: &mut u16,
    hail_minutes: &mut u16,
    env_model: &mut wxdata::hrrr::Model,
    active_contours: &mut std::collections::BTreeSet<crate::app::ContourKind>,
    etop_dbz: &mut f32,
    snow_hours: &mut u16,
    show_tropical: &bool,
    tropical_wind_kt: &mut Option<u8>,
    tropical_surge: &mut bool,
    spaghetti: &mut crate::spaghetti::Spaghetti,
    l3grid_site: Option<&str>,
    // Global models: which one, and how far into its run.
    global_fcst_hour: &mut u16,
    // Model difference: selected field, shared valid time, and both source runs.
    diff_field: &mut crate::fielddiff::DiffField,
    diff_mode: &mut crate::fielddiff::DiffMode,
    diff_valid: Option<&crate::fielddiff::ComparisonTimes>,
    compare_valid: Option<&crate::fielddiff::ComparisonTimes>,
    diff_error: Option<&str>,
    compare_error: Option<&str>,
    // Active one-pane comparison modes (ROADMAP_NEW F6/J4) — read-only here; toggles go through
    // PaletteAction because each also has to maintain mutually-exclusive `fields_on` state.
    blink_compare: bool,
    overlay_compare: bool,
    swipe_compare: bool,
    // Ensemble layer (ROADMAP_NEW F7): what it shows, a one-line status, and the reader's
    // temperature unit for a threshold typed in degrees.
    ensemble: &mut crate::ensemble_layer::EnsembleView,
    ensemble_note: &str,
    temp_unit: crate::settings::TempUnit,
    // Lightning: NLDN averaging window, and whether GLM also polls GOES-West.
    lightning_minutes: &mut u16,
    show_glm: bool,
    glm_goes_west: &mut bool,
    // Satellite: GOES-East or -West for the IR/visible/water-vapor CMIP bands (exclusive,
    // unlike GLM's additive choice above — the two satellites' CONUS scans overlap).
    goes_satellite_west: &mut bool,
    // Spotter Network dots: on-state, and how far from the radar to draw them (0 = whole feed).
    show_spotters: bool,
    spotter_range_km: &mut f64,
    // Where the signature detectors draw their lines; only rendered for the ones that are on.
    detectors: &mut crate::settings::DetectorTuning,
    // One line about the live composite: contributing sites and the age of its oldest scan, or
    // why there isn't one. Radars scan on their own schedules, so a composite is always a little
    // ragged in time and the honest thing is to show by how much.
    mosaic: Option<&str>,
    actions: &mut UiActions,
) {
    use crate::render::FieldLayer as FL;
    let mut changed = false;

    let mut stamped: Vec<_> = on
        .iter()
        .filter_map(|layer| {
            fields
                .get(layer)?
                .stamp
                .as_ref()
                .map(|stamp| (layer.slug(), stamp))
        })
        .collect();
    stamped.sort_by_key(|(slug, _)| *slug);
    for (slug, stamp) in stamped {
        let mismatch = analysis_time.is_some_and(|analysis_time| {
            wxdata::time_align::TimeOffset::between(stamp.valid_time, analysis_time, time_tolerance)
                .outside_tolerance
        });
        let caption = if mismatch {
            format!("⚠ Data source · {slug} · time mismatch")
        } else {
            format!("Data source · {slug}")
        };
        ui.collapsing(caption, |ui| {
            if let Some(field) = FL::from_slug(slug).and_then(FL::descriptor) {
                ui.label(format!(
                    "{} · {} · {:?}",
                    field.name,
                    field.units.symbol(),
                    field.value_kind
                ));
                ui.label(format!(
                    "Missing/no coverage codes: {:?} (masked)",
                    field.missing_values
                ));
            }
            super::data_inspector::show(ui, stamp, analysis_time, time_tolerance);
        });
    }

    let sections = [
        ("Storm cells", filters.show_cells),
        ("Alerts", filters.show_alerts),
        ("Tropical", *show_tropical),
        ("Outlooks", true),
        ("Environment", true),
        (
            "Model comparison",
            on.contains(&FL::ModelDiff) || on.contains(&FL::CompareA) || on.contains(&FL::CompareB),
        ),
        ("Ensemble", on.contains(&FL::Ensemble)),
        ("Lightning", show_glm || on.contains(&FL::Lightning)),
        (
            "QPE accumulation",
            QPE_WINDOWS.iter().any(|(layer, _)| on.contains(layer)),
        ),
        (
            "FLASH rainfall rarity",
            FLASH_ARI_WINDOWS
                .iter()
                .any(|(layer, _)| on.contains(layer)),
        ),
        (
            "MRMS echo tops",
            ECHO_TOP_THRESHOLDS
                .iter()
                .any(|(layer, _)| on.contains(layer)),
        ),
        (
            "MRMS temperature levels",
            ISOTHERM_LEVELS.iter().any(|(layer, _)| on.contains(layer)),
        ),
        (
            "Satellite",
            [
                FL::GoesIr,
                FL::GoesVisible,
                FL::GoesWaterVapor,
                FL::GoesShortwaveIr,
                FL::GoesMidWaterVapor,
                FL::GoesLowWaterVapor,
                FL::GoesDirtyIr,
                FL::GoesDustDiff,
                FL::GoesColdTop,
            ]
            .iter()
            .any(|l| on.contains(l)),
        ),
        ("Spotters", show_spotters),
        (
            "Rotation tracks",
            on.contains(&FL::Rotation) || on.contains(&FL::RotationMidLevel),
        ),
        ("Hail swaths", on.contains(&FL::HailSwath)),
        ("Radar mosaic", on.contains(&FL::Mosaic)),
        ("Nowcast", filters.show_nowcast),
        ("Max/min trail", filters.show_trail),
        ("Snowfall", on.contains(&FL::SnowAnalysis)),
        (
            "Derived radar",
            [FL::VilLocal, FL::VilDensity, FL::EtopLocal]
                .iter()
                .any(|l| on.contains(l)),
        ),
        (
            "Detectors",
            filters.show_tds
                || filters.show_couplets
                || filters.show_tbss
                || filters.show_zdr_columns,
        ),
        (
            "Level 3 grids",
            [FL::Vil, FL::EchoTops, FL::Hca]
                .iter()
                .any(|l| on.contains(l)),
        ),
    ];
    let id = ui.id().with("layer_settings_section");
    let remembered = ui.ctx().data_mut(|d| d.get_temp::<&'static str>(id));
    let mut section = remembered
        .filter(|name| sections.iter().any(|(s, on)| s == name && *on))
        .unwrap_or(sections.iter().find(|(_, on)| *on).unwrap().0);
    ui.spacing_mut().item_spacing.y = 8.0;
    egui::ComboBox::from_id_salt("settings_for")
        .width(ui.available_width() - 8.0)
        .selected_text(section)
        .show_ui(ui, |ui| {
            for (name, visible) in sections {
                if visible {
                    ui.selectable_value(&mut section, name, name);
                }
            }
        })
        .response
        .on_hover_text("Choose a layer to adjust");
    ui.ctx().data_mut(|d| d.insert_temp(id, section));
    ui.add_space(4.0);
    let showing_compare = on.contains(&FL::CompareA) || on.contains(&FL::CompareB);
    // Blinking also keeps exactly one of CompareA/CompareB in `fields_on` (see
    // `render_pane`'s own comment), so `showing_compare` alone can't tell true two-pane side by
    // side apart from one pane alternating between the two on a timer.
    let in_side_by_side = showing_compare && !blink_compare && !overlay_compare && !swipe_compare;
    if section == "QPE accumulation" {
        qpe_window_control(ui, on, actions);
    }
    if section == "FLASH rainfall rarity" {
        flash_ari_window_control(ui, on, actions);
    }
    if section == "MRMS echo tops" {
        echo_top_threshold_control(ui, on, actions);
    }
    if section == "MRMS temperature levels" {
        isotherm_level_control(ui, on, actions);
    }
    if section == "Model comparison" && (on.contains(&FL::ModelDiff) || showing_compare) {
        let (a, b) = diff_field.pair();
        ui.horizontal_wrapped(|ui| {
            ui.label("Field:");
            for f in crate::fielddiff::DiffField::ALL {
                changed |= ui.selectable_value(diff_field, f, f.label()).changed();
            }
        });
        match diff_field.lead_hours() {
            Some((max, step)) => {
                // The regional pair stops at 18 h; a global-sized hour would ask for a file that
                // does not exist.
                *global_fcst_hour = (*global_fcst_hour).min(max);
                ui.horizontal(|ui| {
                    ui.label("Forecast hour:");
                    changed |= ui
                        .add(
                            egui::Slider::new(global_fcst_hour, 0..=max)
                                .step_by(f64::from(step))
                                .suffix(" h"),
                        )
                        .changed();
                });
            }
            None => {
                ui.weak("Fixed at the analysis hour: the latest run against the one before it.");
            }
        }
        if on.contains(&FL::ModelDiff) {
            ui.horizontal_wrapped(|ui| {
                ui.label("Difference:");
                for mode in crate::fielddiff::DiffMode::ALL {
                    changed |= ui.selectable_value(diff_mode, mode, mode.label()).changed();
                }
            });
            ui.weak(match diff_mode {
                crate::fielddiff::DiffMode::Signed => format!(
                    "{a} minus {b}, in {}. Red = {a} higher, blue = {b} higher; where they agree, nothing is drawn.",
                    diff_field.units()
                ),
                crate::fielddiff::DiffMode::Absolute => format!(
                    "Magnitude of the {a}/{b} difference, in {}. Color shows how far apart they are without direction; agreement is not drawn.",
                    diff_field.units()
                ),
                crate::fielddiff::DiffMode::Disagreement => {
                    let (_, deadband) = diff_field.range();
                    format!(
                        "Categorical mask: magenta where |{a} − {b}| exceeds {deadband:.1} {}; agreement and missing data are not drawn.",
                        diff_field.units()
                    )
                }
            });
            ui.weak(valid_time_note(diff_valid, diff_error, a, b));
        }
        if showing_compare {
            let label = diff_field.label();
            if blink_compare {
                ui.weak(format!(
                    "Blinking between {a}'s own {label} and {b}'s own {label} every 1.5 s — same \
                     scale, so a difference in the field itself is easy to spot by eye."
                ));
            } else if overlay_compare {
                ui.weak(format!(
                    "{a}'s own {label} at full opacity with {b} overlaid at 50% — one pane shows \
                     displacement and shape differences directly on the same scale."
                ));
            } else if swipe_compare {
                ui.weak(format!(
                    "{a}'s own {label} on the left and {b}'s on the right — drag the divider to \
                     compare displacement and shape at the same scale."
                ));
            } else {
                ui.weak(format!(
                    "Pane A: {a}'s own {label}. Pane B: {b}'s own {label} — same scale, so a \
                     difference in the field itself (not just where they disagree) is easy to spot \
                     by eye."
                ));
            }
            ui.weak(valid_time_note(compare_valid, compare_error, a, b));
        }
        // A run-to-run field has no distinct "previous run" layer of its own yet, so side by side
        // would draw the same current-run layer in both panes — hidden rather than shipped
        // half-working (see `DiffField::supports_side_by_side`'s own doc comment).
        if diff_field.supports_side_by_side() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if in_side_by_side {
                        "Rearrange panes"
                    } else {
                        "View side by side (2 panes)"
                    })
                    .on_hover_text(
                        "Puts one model's own field in each of two panes with linked cameras, \
                         instead of one subtracted layer",
                    )
                    .clicked()
                {
                    actions.palette = Some(crate::app::PaletteAction::CompareInPanes);
                }
                if ui
                    .button(if blink_compare {
                        "Stop blinking"
                    } else {
                        "Blink A/B"
                    })
                    .on_hover_text(
                        "Alternate this one pane between each model's own field on a timer, \
                         instead of splitting into two panes",
                    )
                    .clicked()
                {
                    actions.palette = Some(crate::app::PaletteAction::ToggleBlinkCompare);
                }
                if ui
                    .button(if overlay_compare {
                        "Stop overlay"
                    } else {
                        "Overlay A/B"
                    })
                    .on_hover_text("Draw model A normally and model B at 50% opacity in this pane")
                    .clicked()
                {
                    actions.palette = Some(crate::app::PaletteAction::ToggleCompareOverlay);
                }
                if ui
                    .button(if swipe_compare {
                        "Stop swipe"
                    } else {
                        "Swipe A/B"
                    })
                    .on_hover_text("Split this pane between model A and B with a draggable divider")
                    .clicked()
                {
                    actions.palette = Some(crate::app::PaletteAction::ToggleCompareSwipe);
                }
            });
        }
    }

    if section == "Ensemble" && on.contains(&FL::Ensemble) {
        use crate::ensemble_layer::StatKind;
        use wxdata::ensemble::EnsembleField;
        ui.horizontal_wrapped(|ui| {
            ui.label("Field:");
            for f in EnsembleField::ALL {
                if ui
                    .selectable_label(ensemble.field == f, f.label())
                    .clicked()
                {
                    ensemble.set_field(f);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Show:");
            for kind in StatKind::ALL {
                ui.selectable_value(&mut ensemble.kind, kind, kind.label());
            }
        });
        if ensemble.kind == StatKind::Probability {
            let (mut shown, unit) = ensemble.threshold_display(temp_unit);
            ui.horizontal(|ui| {
                ui.label("Chance above:");
                let speed = (shown.abs() * 0.01).max(0.5);
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .speed(speed)
                            .suffix(format!(" {unit}")),
                    )
                    .changed()
                {
                    ensemble.set_threshold_display(shown, temp_unit);
                }
                if ui.small_button("Reset").clicked() {
                    ensemble.threshold = ensemble.field.default_threshold();
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Forecast hour:");
            changed |= ui
                .add(
                    egui::Slider::new(global_fcst_hour, 0..=120)
                        .step_by(3.0)
                        .suffix(" h"),
                )
                .on_hover_text("Three-hourly from the newest complete GEFS cycle")
                .changed();
        });
        ui.weak(match ensemble.kind {
            StatKind::Mean => "Average of the 31 members. Smooths out detail no single run can be trusted on.",
            StatKind::Spread => "How far the members disagree (standard deviation). High spread means low confidence.",
            StatKind::Min | StatKind::Max => "The coolest/lowest or warmest/highest any member forecast at each point.",
            StatKind::P10 | StatKind::P90 => "One member in ten is beyond this value — a plausible low or high end.",
            StatKind::Probability => "Share of the 31 members above the threshold. It is a fraction of runs, not a calibrated probability.",
        });
        ui.weak(ensemble_note);
    }

    if section == "Lightning" && on.contains(&FL::Lightning) {
        ui.horizontal(|ui| {
            ui.label("CG density window:");
            for m in [1u16, 5, 15, 30] {
                changed |= ui
                    .selectable_value(lightning_minutes, m, format!("{m}m"))
                    .changed();
            }
        });
        ui.weak("NLDN = cloud-to-ground only; GLM = total lightning (optical, in-cloud included).");
    }

    if section == "Lightning" && show_glm {
        changed |= crate::ui::style::toggle(ui, glm_goes_west, "Include GOES-West")
            .on_hover_text("Adds GOES-18 so the Pacific and the west coast are covered too")
            .changed();
    }

    if section == "Satellite" {
        changed |= crate::ui::style::toggle(ui, goes_satellite_west, "Use GOES-West")
            .on_hover_text(
                "GOES-18 instead of GOES-East — covers the Pacific and the western half of the \
                 country. The two satellites' CONUS scans overlap, so this replaces the image \
                 rather than adding to it.",
            )
            .changed();
    }

    if section == "Spotters" && show_spotters {
        ui.horizontal(|ui| {
            ui.label("Spotters within:");
            ui.add(
                egui::DragValue::new(spotter_range_km)
                    .range(0.0..=5000.0)
                    .speed(10.0)
                    .max_decimals(0)
                    .suffix(" km"),
            )
            .on_hover_text("Distance from the active radar. 0 draws the whole national feed.");
        });
    }

    if section == "Outlooks" {
        // SPC outlook: a four-way day selector whose own "Off" is the off-state, so it can't wear the
        // registry's ON/OFF pill. It lives here rather than in the layer list.
        // Days 4–8 are SPC's experimental severe probability, one layer per day; the row wraps
        // rather than growing a second control for "which kind of day this is".
        ui.label("SPC Outlook");
        egui::ComboBox::from_id_salt("outlook_day")
            .width(ui.available_width() - 8.0)
            .selected_text(if filters.outlook_day == 0 {
                "Off".to_string()
            } else {
                format!("Day {}", filters.outlook_day)
            })
            .show_ui(ui, |ui| {
                for day in 0u8..=8 {
                    let label = if day == 0 {
                        "Off".to_string()
                    } else {
                        format!("Day {day}")
                    };
                    changed |= ui
                        .selectable_value(&mut filters.outlook_day, day, label)
                        .changed();
                }
            });
        // Day-1 hazard sub-select (probabilistic tornado/wind/hail); Days 2–3 are categorical only.
        if filters.outlook_day == 1 {
            ui.indent("outlook_kind", |ui| {
                ui.label("Hazard");
                egui::ComboBox::from_id_salt("outlook_hazard")
                    .width(ui.available_width() - 8.0)
                    .selected_text(filters.outlook_kind.label())
                    .show_ui(ui, |ui| {
                        for kind in wxdata::spc::OutlookKind::ALL {
                            if ui
                                .selectable_value(&mut filters.outlook_kind, kind, kind.label())
                                .changed()
                            {
                                actions.outlook_kind_changed = true;
                                changed = true;
                            }
                        }
                    });
            });
        }

        // Excessive Rainfall Outlook: the flood half of the day, directly under the severe half.
        ui.label("Rainfall outlook");
        egui::ComboBox::from_id_salt("ero_day")
            .width(ui.available_width() - 8.0)
            .selected_text(if filters.ero_day == 0 {
                "Off".to_string()
            } else {
                format!("Day {}", filters.ero_day)
            })
            .show_ui(ui, |ui| {
                for day in 0u8..=3 {
                    let label = if day == 0 {
                        "Off".to_string()
                    } else {
                        format!("Day {day}")
                    };
                    if ui
                        .selectable_value(&mut filters.ero_day, day, label)
                        .changed()
                    {
                        actions.ero_day_changed = true;
                        changed = true;
                    }
                }
            });

        // Winter Storm Severity Index: same off-plus-three-days shape as the outlook selector.
        ui.label("Winter impacts");
        egui::ComboBox::from_id_salt("wssi_day")
            .width(ui.available_width() - 8.0)
            .selected_text(if filters.wssi_day == 0 {
                "Off".to_string()
            } else {
                format!("Day {}", filters.wssi_day)
            })
            .show_ui(ui, |ui| {
                for day in 0u8..=3 {
                    let label = if day == 0 {
                        "Off".to_string()
                    } else {
                        format!("Day {day}")
                    };
                    if ui
                        .selectable_value(&mut filters.wssi_day, day, label)
                        .changed()
                    {
                        actions.wssi_day_changed = true;
                        changed = true;
                    }
                }
            });

        // Fire Weather Outlook: categorical risk + dry thunderstorm hazard together. Day 1-2
        // only — SPC's own Day 3-8 product splits into different hazard layers entirely rather
        // than publishing this same categorical risk further out (see `wxdata::firewx`).
        ui.label("Fire weather outlook");
        egui::ComboBox::from_id_salt("fire_day")
            .width(ui.available_width() - 8.0)
            .selected_text(if filters.fire_day == 0 {
                "Off".to_string()
            } else {
                format!("Day {}", filters.fire_day)
            })
            .show_ui(ui, |ui| {
                for day in 0u8..=2 {
                    let label = if day == 0 {
                        "Off".to_string()
                    } else {
                        format!("Day {day}")
                    };
                    if ui
                        .selectable_value(&mut filters.fire_day, day, label)
                        .changed()
                    {
                        actions.fire_day_changed = true;
                        changed = true;
                    }
                }
            });
    }

    if section == "Environment" {
        // Where the environment fields and contours come from. RAP f00 is an analysis of what the
        // atmosphere is doing now (assimilated obs, 13 km) rather than an HRRR forecast at hour zero —
        // the thing people mean by "mesoanalysis". Labelled honestly, coarser grid and all.
        ui.weak(format!(
            "Model: {} \u{2014} pick it under Model forecast",
            env_model.label()
        ));

        // Model contours (isolines) — MSLP / 2 m temp / dewpoint / SB-CAPE / 0-3 km SRH / … .
        // Several can be on at once (e.g. MSLP and CAPE together), so this is a checklist rather
        // than an exclusive picker.
        ui.label("Contours");
        egui::ComboBox::from_id_salt("environment_contours")
            .width(ui.available_width() - 8.0)
            .selected_text(crate::app::summarize_contours(active_contours))
            .show_ui(ui, |ui| {
                for k in crate::app::ContourKind::ALL {
                    if k == crate::app::ContourKind::Off {
                        continue; // "no kinds checked" already means Off
                    }
                    if (k == crate::app::ContourKind::Stp || k == crate::app::ContourKind::StpEff)
                        && !stp_source(*env_model)
                    {
                        continue; // no LCL height in these files
                    }
                    let mut on = active_contours.contains(&k);
                    if ui.checkbox(&mut on, k.label()).changed() {
                        if on {
                            active_contours.insert(k);
                        } else {
                            active_contours.remove(&k);
                        }
                    }
                }
            })
            .response
            .on_hover_text("Draw one or more surface fields as labeled contour lines (f00)");
    }

    // Everything below belongs to a layer that has to be on for it to mean anything.
    let header = |ui: &mut egui::Ui, text: &str| {
        if text != section && !(section == "Alerts" && text == "NWS Alerts") {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(text).small().strong());
        }
    };

    if section == "Rotation tracks"
        && (on.contains(&FL::Rotation) || on.contains(&FL::RotationMidLevel))
    {
        header(ui, "Rotation tracks");
        ui.horizontal(|ui| {
            ui.label("Window:");
            let mut dur = false;
            egui::ComboBox::from_id_salt("mrms_rotation_window")
                .selected_text(rotation_window_label(*rotation_minutes))
                .show_ui(ui, |ui| {
                    for minutes in wxdata::mrms::ROTATION_WINDOWS {
                        dur |= ui
                            .selectable_value(
                                rotation_minutes,
                                minutes,
                                rotation_window_label(minutes),
                            )
                            .changed();
                    }
                })
                .response
                .named("MRMS rotation track window")
                .on_hover_text("Applies to both 0–2 km and 3–6 km AGL tracks in all panes");
            // Duration change → refetch both altitude bands on their new window.
            if dur {
                for layer in [FL::Rotation, FL::RotationMidLevel] {
                    if let Some(s) = fields.get_mut(&layer) {
                        s.last_fetch = None;
                    }
                }
            }
        });
    }

    if section == "Hail swaths" && on.contains(&FL::HailSwath) {
        header(ui, "Hail swaths");
        ui.horizontal(|ui| {
            ui.label("Window:");
            let mut dur = false;
            for (m, label) in [
                (30u16, "30m"),
                (60, "1h"),
                (120, "2h"),
                (360, "6h"),
                (1440, "24h"),
            ] {
                dur |= ui.selectable_value(hail_minutes, m, label).changed();
            }
            if dur {
                if let Some(s) = fields.get_mut(&FL::HailSwath) {
                    s.last_fetch = None;
                }
            }
        });
    }

    if section == "Radar mosaic" && on.contains(&FL::Mosaic) {
        header(ui, "Radar mosaic");
        if let Some(m) = mosaic {
            ui.weak(m);
        }
    }

    if section == "Storm cells" && filters.show_cells {
        header(ui, "Storm cells");
        crate::ui::style::toggle(ui, &mut filters.show_tracks, "Forecast tracks")
            .on_hover_text("15/30/45/60-min projected storm positions");
        crate::ui::style::toggle(ui, &mut filters.show_arrival_cones, "Arrival-time cones")
            .on_hover_text("Project cell motion forward + ETA to your saved markers");
    }

    if section == "Max/min trail" && filters.show_trail {
        header(ui, "Max/min trail");
        ui.horizontal(|ui| {
            ui.label("Window:");
            for m in [15u16, 30, 60, 120] {
                ui.selectable_value(&mut filters.trail_window_min, m, format!("{m}m"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Keep:");
            ui.selectable_value(&mut filters.trail_keep_min, false, "Maximum");
            ui.selectable_value(&mut filters.trail_keep_min, true, "Minimum");
        });
        if ui
            .button("Reset at playhead")
            .on_hover_text(
                "Discard the running trail and recompute the cached window ending at the selected time",
            )
            .clicked()
        {
            actions.reset_trail = true;
        }
        if !filters.trail_status.is_empty() {
            ui.small(filters.trail_status.as_str());
        }
        ui.small(
            "Drawn in place of the radar for this product and tilt. The value threshold and \
             smoothing apply to the trail. Built from volumes already in the loop, ending at \
             the playhead.",
        );
    }

    if section == "Nowcast" && filters.show_nowcast {
        header(ui, "Nowcast");
        ui.horizontal(|ui| {
            ui.label("Lead:");
            for m in [15u8, 30, 45, 60, 90, 120] {
                ui.selectable_value(&mut filters.nowcast_lead_min, m, format!("{m}m"));
            }
        });
        if filters.nowcast_lead_min > 45 {
            ui.small(
                "Past 45 minutes this is extrapolation, not forecasting — it moves the echo \
                 that exists and cannot grow or decay it. A model's reflectivity forecast is \
                 the answer for an hour or more.",
            );
        }
    }

    if section == "Alerts" && filters.show_alerts {
        header(ui, "NWS Alerts");
        for cat in Category::ALL {
            changed |=
                crate::ui::style::toggle(ui, &mut filters.alert_cats[cat.index()], cat.label())
                    .changed();
        }
    }

    if section == "Tropical" && *show_tropical {
        header(ui, "Tropical");
        ui.label("Wind field");
        egui::ComboBox::from_id_salt("tropical_wind")
            .width(ui.available_width() - 8.0)
            .selected_text(
                tropical_wind_kt.map_or_else(|| "Off".to_string(), |kt| format!("{kt} kt")),
            )
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(tropical_wind_kt, None, "Off").changed();
                for kt in [34u8, 50, 64] {
                    changed |= ui
                        .selectable_value(tropical_wind_kt, Some(kt), format!("{kt} kt"))
                        .on_hover_text("How far out the forecast wind of that strength reaches")
                        .changed();
                }
            });
        changed |= crate::ui::style::toggle(ui, tropical_surge, "Potential storm surge")
            .on_hover_text(
                "How deep water could get above ground if the peak surge arrives at high tide",
            )
            .changed();
        crate::ui::style::toggle(ui, &mut spaghetti.enabled, "Model tracks (spaghetti)")
            .on_hover_text(
                "Every model's track for every active storm and invest, with the observed track",
            );
        if spaghetti.enabled {
            crate::ui::style::toggle(ui, &mut spaghetti.best_track, "Observed track");
        }
        if ui
            .button("Models & advisories\u{2026}")
            .on_hover_text("Pick models, read the intensity guidance and NHC's advisories")
            .clicked()
        {
            spaghetti.open_window = true;
        }
    }

    if section == "Snowfall" && on.contains(&FL::SnowAnalysis) {
        header(ui, "Snowfall analysis");
        ui.horizontal(|ui| {
            ui.label("Window:");
            for h in wxdata::nohrsc::DURATIONS {
                changed |= ui
                    .selectable_value(snow_hours, h, format!("{h}h"))
                    .changed();
            }
        });
    }

    if section == "Derived radar"
        && [FL::VilLocal, FL::VilDensity, FL::EtopLocal]
            .iter()
            .any(|l| on.contains(l))
    {
        header(ui, "Derived products");
        ui.horizontal(|ui| {
            ui.label("Echo top:");
            changed |= ui
                .add(egui::Slider::new(etop_dbz, 5.0..=50.0).suffix(" dBZ"))
                .on_hover_text("Reflectivity that counts as the storm top (18.5 = NWS EET)")
                .changed();
        });
    }

    // Detector thresholds. Each block only appears with its own detector on, and the defaults are
    // what the detectors shipped with — the reset button is there because a slider you can't get
    // back from is worse than no slider.
    if section == "Detectors" && filters.show_tds {
        header(ui, "Debris signature (TDS)");
        let mut pct = (detectors.tds_min_confidence * 100.0).round();
        if ui
            .add(
                egui::Slider::new(&mut pct, 0.0..=90.0)
                    .text("Minimum confidence")
                    .suffix("%"),
            )
            .on_hover_text(
                "Hide debris signatures below this confidence, and keep them from raising an \
                 alert. Confidence rises with a deep dip in correlation coefficient that stands \
                 out from its surroundings, strong reflectivity, a compact size, and a signature \
                 that repeats up through the tilts. A rotation couplet beside it raises it \
                 further; none, where velocity was scanned, costs a fifth. One tilt alone never \
                 exceeds 60%. Starts at 60%: on the archived-event backtest that caught as \
                 many tornadoes as 50% with far fewer false alarms.",
            )
            .changed()
        {
            detectors.tds_min_confidence = pct / 100.0;
        }
        ui.weak("Each marker shows its confidence. Raise this to cut down on doubtful ones.");
    }
    if section == "Detectors" && filters.show_couplets {
        header(ui, "Rotation couplets");
        let mut pct = (detectors.rotation_min_confidence * 100.0).round();
        if ui
            .add(
                egui::Slider::new(&mut pct, 0.0..=90.0)
                    .text("Minimum confidence")
                    .suffix("%"),
            )
            .on_hover_text(
                "Hide rotation couplets below this confidence, and keep them from raising an \
                 alert. Confidence rises with strong gate-to-gate shear, a sizeable cluster of \
                 gates, and a couplet that repeats up through the tilts; it fades with range. \
                 One tilt alone never exceeds 50%. Starts at 50%: rotation comes before \
                 debris, so this errs toward catching more.",
            )
            .changed()
        {
            detectors.rotation_min_confidence = pct / 100.0;
        }
        ui.weak("Each marker shows its confidence; hover it for the working.");
    }
    if section == "Detectors" && filters.show_tbss {
        header(ui, "Hail spike (TBSS)");
        ui.add(
            egui::Slider::new(&mut detectors.tbss_core_dbz, 50.0..=70.0)
                .text("Core")
                .suffix(" dBZ"),
        )
        .on_hover_text("How strong the core must be before a spike behind it is looked for");
    }
    if section == "Detectors" && filters.show_zdr_columns {
        header(ui, "ZDR columns");
        ui.add(
            egui::Slider::new(&mut detectors.zdr_min_db, 0.5..=3.0)
                .text("Minimum ZDR")
                .suffix(" dB"),
        );
        ui.add(
            egui::Slider::new(&mut detectors.zdr_min_depth_km, 0.5..=3.0)
                .text("Depth above freezing")
                .suffix(" km"),
        );
    }
    if section == "Lightning" && show_glm {
        header(ui, "Flash-extent density");
        ui.add(
            egui::Slider::new(&mut detectors.glm_fed_cell_deg, 0.02..=0.2)
                .text("Cell size")
                .suffix("°"),
        )
        .on_hover_text("Grid resolution: 0.05° is about 5 km");
        ui.add(
            egui::Slider::new(&mut detectors.glm_fed_window_min, 5..=30)
                .text("Window")
                .suffix(" min"),
        );
        ui.weak("Takes effect on the next flash-density refresh.");
    }
    if (section == "Detectors" || section == "Lightning")
        && (filters.show_tds
            || filters.show_couplets
            || filters.show_tbss
            || filters.show_zdr_columns
            || show_glm)
        && ui.button("Reset detector thresholds").clicked()
    {
        *detectors = crate::settings::DetectorTuning::default();
    }

    if section == "Level 3 grids"
        && [FL::Vil, FL::EchoTops, FL::Hca]
            .iter()
            .any(|l| on.contains(l))
    {
        header(ui, "Level 3 grids");
        ui.weak(format!("Site: {}", l3grid_site.unwrap_or("—")));
    }

    actions.overlays_changed |= changed;
}

#[cfg(test)]
mod catalog_choice_tests {
    use super::*;
    use crate::render::FieldLayer as FL;

    #[test]
    fn picker_preserves_other_layers_and_existing_qpe_slugs() {
        let mut on = [FL::Mrms, FL::Qpe1h, FL::Qpe24h].into_iter().collect();
        assert!(select_qpe_window(&mut on, FL::Qpe6h));
        assert_eq!(on.len(), 2);
        assert!(on.contains(&FL::Mrms));
        assert!(on.contains(&FL::Qpe6h));
        assert_eq!(FL::Qpe6h.slug(), "qpe6h");
        assert!(!select_qpe_window(&mut on, FL::Mesh));
        assert_eq!(on.len(), 2);
    }

    #[test]
    fn echo_top_picker_changes_only_its_own_threshold_group() {
        let mut on = [FL::Mrms, FL::Qpe6h, FL::MrmsEchoTop18, FL::MrmsEchoTop50]
            .into_iter()
            .collect();
        assert!(select_echo_top_threshold(&mut on, FL::MrmsEchoTop60));
        assert_eq!(on.len(), 3);
        assert!(on.contains(&FL::Mrms));
        assert!(on.contains(&FL::Qpe6h));
        assert!(on.contains(&FL::MrmsEchoTop60));
        assert!(!select_echo_top_threshold(&mut on, FL::Qpe1h));
        assert_eq!(on.len(), 3);
    }

    #[test]
    fn isotherm_picker_changes_only_its_own_temperature_group() {
        let mut on = [
            FL::Mrms,
            FL::MrmsEchoTop50,
            FL::MrmsRefl0c,
            FL::MrmsReflM10c,
        ]
        .into_iter()
        .collect();
        assert!(select_isotherm_level(&mut on, FL::MrmsReflM20c));
        assert_eq!(on.len(), 3);
        assert!(on.contains(&FL::Mrms));
        assert!(on.contains(&FL::MrmsEchoTop50));
        assert!(on.contains(&FL::MrmsReflM20c));
        assert!(!select_isotherm_level(&mut on, FL::MrmsEchoTop18));
        assert_eq!(on.len(), 3);
    }

    #[test]
    fn flash_ari_picker_preserves_other_hydrology_layers() {
        let mut on = [FL::Qpe6h, FL::FlashFlood, FL::FlashFlood3h]
            .into_iter()
            .collect();
        assert!(select_flash_ari_window(&mut on, FL::FlashFloodMax));
        assert_eq!(on.len(), 2);
        assert!(on.contains(&FL::Qpe6h));
        assert!(on.contains(&FL::FlashFloodMax));
        assert!(!select_flash_ari_window(&mut on, FL::Qpe1h));
        assert_eq!(on.len(), 2);
    }
}
