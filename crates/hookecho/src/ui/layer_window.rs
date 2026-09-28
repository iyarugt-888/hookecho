//! The Layer Manager: styling for the imported GIS reference plus per-placefile enable, paint
//! order, and opacity.
//!
//! `settings.placefiles` order *is* the paint order (see `visible_placefile_items`), so the
//! ↑/↓ buttons here reorder the list directly. Field layers keep their fixed `DRAW_ORDER`;
//! field-layer opacity rides in the grid uniform's spare word (`shaders/mrms.wgsl`), so those
//! sliders are free — no LUT re-bake.

use crate::render::FieldLayer;
use crate::settings::Settings;

/// What the Layer Manager shows about the imported GIS layer beyond its settings.
pub(crate) struct Imported<'a> {
    /// Every attribute name in the file: the choices for labels, colours and times.
    pub keys: &'a [String],
    /// The colour-by legend, when one is on.
    pub legend: Option<&'a crate::gis_import::Legend>,
    /// With a time attribute mapped: how many features are valid at the view's time, of all.
    pub time_count: Option<(usize, usize)>,
}

/// One attribute picker: "None" or any of `keys`, bound to `value`.
fn attribute_combo(
    ui: &mut egui::Ui,
    id: &str,
    none: &str,
    keys: &[String],
    value: &mut Option<String>,
) -> bool {
    let mut changed = false;
    let current = value.clone();
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.as_deref().unwrap_or(none))
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), none).clicked() {
                *value = None;
                changed = true;
            }
            for key in keys {
                let on = current.as_deref() == Some(key.as_str());
                if ui.selectable_label(on, key).clicked() {
                    *value = Some(key.clone());
                    changed = true;
                }
            }
        });
    changed
}

/// Show the window. `active` is the field layers currently painting, with their display names
/// (only those get a slider).
/// Returns `true` if anything changed (the caller bumps the overlay generation so the tessellated
/// geometry rebuilds).
pub(crate) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    active: &[(FieldLayer, String)],
    imported: &Imported,
    drawer: &mut crate::ui::drawer::Drawer,
) -> bool {
    if !*open {
        return false;
    }
    let mut changed = false;
    let mut win_open = *open;
    let Some(window) = drawer.page_sized(
        ctx,
        "Layer Manager",
        &mut win_open,
        false,
        420.0,
        egui::Window::new("Layer Manager"),
    ) else {
        *open = win_open;
        return false;
    };
    window.show(ctx, |ui| {
        if !active.is_empty() {
            ui.label(egui::RichText::new("Field layers").strong());
            for (layer, name) in active {
                let op = settings.field_opacity.entry(*layer).or_insert(1.0);
                ui.horizontal(|ui| {
                    ui.label(name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        changed |= ui
                            .add(egui::Slider::new(op, 0.05..=1.0).show_value(false))
                            .on_hover_text(format!("Opacity {:.0}%", *op * 100.0))
                            .changed();
                    });
                });
            }
            ui.separator();
        }
        if let Some(source) = settings.imported_gis.clone() {
            ui.label(egui::RichText::new("Imported GIS").strong());
            let name = source
                .rsplit(['/', '\\'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(&source);
            ui.weak(name).on_hover_text(source);
            ui.horizontal(|ui| {
                ui.label("Color");
                changed |= ui
                    .color_edit_button_srgb(&mut settings.imported_gis_style.color)
                    .on_hover_text("Color for imported polygons, lines, and points")
                    .changed();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Reset style")
                        .on_hover_text("Restore the neutral-blue imported-layer style")
                        .clicked()
                    {
                        settings.imported_gis_style = Default::default();
                        changed = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Outline");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.imported_gis_style.stroke_width, 0.5..=8.0)
                            .suffix(" px")
                            .max_decimals(1),
                    )
                    .on_hover_text("Width for imported polygon edges, lines, and point symbols")
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Opacity");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.imported_gis_style.opacity, 0.05..=1.0)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                    )
                    .on_hover_text(format!(
                        "Opacity {:.0}%",
                        settings.imported_gis_style.opacity * 100.0
                    ))
                    .changed();
            });
            let keys = imported.keys;
            ui.horizontal(|ui| {
                ui.label("Label")
                    .on_hover_text("Label each imported feature with this attribute's value");
                changed |= attribute_combo(
                    ui,
                    "imported_gis_label",
                    "None",
                    keys,
                    &mut settings.imported_gis_label,
                );
            });
            ui.horizontal(|ui| {
                ui.label("Color by").on_hover_text(
                    "Color features by an attribute: a ramp for numbers, a palette for categories",
                );
                changed |= attribute_combo(
                    ui,
                    "imported_gis_color_by",
                    "None (one color)",
                    keys,
                    &mut settings.imported_gis_color_by,
                );
            });
            if settings.imported_gis_color_by.is_some() {
                if let Some(legend) = imported.legend {
                    color_legend(ui, legend);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Valid from").on_hover_text(
                    "Show each feature only from the time in this attribute, following the \
                     timeline",
                );
                changed |= attribute_combo(
                    ui,
                    "imported_gis_time_start",
                    "Always",
                    keys,
                    &mut settings.imported_gis_time_start,
                );
            });
            ui.horizontal(|ui| {
                ui.label("Valid until").on_hover_text(
                    "Hide each feature from the time in this attribute, following the timeline",
                );
                changed |= attribute_combo(
                    ui,
                    "imported_gis_time_end",
                    "Always",
                    keys,
                    &mut settings.imported_gis_time_end,
                );
            });
            if let Some((shown, total)) = imported.time_count {
                ui.weak(format!(
                    "{shown} of {total} features valid at the view's time"
                ));
            }
            changed |= ui
                .checkbox(
                    &mut settings.imported_gis_below,
                    "Draw under warnings, watches and outlooks",
                )
                .on_hover_text(
                    "Paint the imported polygons beneath the official products instead of over them; clicks prefer the official shape either way",
                )
                .changed();
            ui.horizontal(|ui| {
                ui.label("Show from zoom");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.imported_gis_style.min_zoom, 0.0..=14.0)
                            .step_by(0.5)
                            .custom_formatter(|v, _| {
                                if v <= 0.0 {
                                    "always".to_string()
                                } else {
                                    format!("{v:.1}")
                                }
                            }),
                    )
                    .on_hover_text(
                        "Hide the imported layer when zoomed out past this (about 4 is the \
                         whole U.S., 7 a state, 10 a county)",
                    )
                    .changed();
            });
            ui.weak("Color, outline and opacity apply to every geometry; Color by recolors each feature.");
            ui.separator();
        }
        if settings.placefiles.is_empty() {
            if settings.imported_gis.is_none() && active.is_empty() {
                ui.weak("No configurable layers are active.");
            }
            return;
        }
        ui.label(egui::RichText::new("Placefiles").strong());
        ui.weak("Top of the list paints first (underneath).");
        ui.separator();
        let n = settings.placefiles.len();
        let mut swap: Option<(usize, usize)> = None;
        for i in 0..n {
            let cfg = &mut settings.placefiles[i];
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut cfg.enabled, "").changed();
                // The URL's file name is the readable part; the full URL is the tooltip.
                let name = cfg.url.rsplit('/').next().unwrap_or(&cfg.url).to_string();
                ui.label(egui::RichText::new(name).strong())
                    .on_hover_text(&cfg.url);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(i + 1 < n, egui::Button::new("▼"))
                        .on_hover_text("Paint later (on top)")
                        .clicked()
                    {
                        swap = Some((i, i + 1));
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("▲"))
                        .on_hover_text("Paint earlier (underneath)")
                        .clicked()
                    {
                        swap = Some((i, i - 1));
                    }
                    changed |= ui
                        .add(egui::Slider::new(&mut cfg.opacity, 0.05..=1.0).show_value(false))
                        .on_hover_text(format!("Opacity {:.0}%", cfg.opacity * 100.0))
                        .changed();
                });
            });
        }
        if let Some((a, b)) = swap {
            settings.placefiles.swap(a, b);
            changed = true;
        }
    });
    *open = win_open;
    changed
}

/// The imported layer's colour key: a gradient bar with its range, or a swatch per category.
fn color_legend(ui: &mut egui::Ui, legend: &crate::gis_import::Legend) {
    use crate::gis_import::Legend;
    let swatch = |ui: &mut egui::Ui, [r, g, b]: [u8; 3]| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
    };
    match legend {
        Legend::Graduated { min, max } => {
            ui.horizontal(|ui| {
                ui.weak(format!("{min}"));
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(140.0, 10.0), egui::Sense::hover());
                let steps = 28;
                for i in 0..steps {
                    let t = i as f64 / (steps - 1) as f64;
                    let [r, g, b] = crate::gis_import::ramp(t);
                    let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
                    let x1 = rect.left() + rect.width() * (i + 1) as f32 / steps as f32;
                    ui.painter().rect_filled(
                        egui::Rect::from_x_y_ranges(x0..=x1, rect.y_range()),
                        0.0,
                        egui::Color32::from_rgb(r, g, b),
                    );
                }
                ui.weak(format!("{max}"));
            });
        }
        Legend::Categories { values, others } => {
            ui.horizontal_wrapped(|ui| {
                for (value, color) in values {
                    swatch(ui, *color);
                    ui.label(egui::RichText::new(value).small());
                    ui.add_space(4.0);
                }
                if *others > 0 {
                    swatch(ui, [150, 150, 150]);
                    ui.label(egui::RichText::new(format!("{others} more")).small());
                }
            });
        }
    }
}
