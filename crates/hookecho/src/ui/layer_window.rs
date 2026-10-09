//! The Layer Manager: styling for the imported GIS reference plus per-placefile enable, paint
//! order, and opacity.
//!
//! `settings.placefiles` order *is* the paint order (see `visible_placefile_items`), so the
//! ↑/↓ buttons here reorder the list directly. Field layers keep their fixed `DRAW_ORDER`;
//! field-layer opacity rides in the grid uniform's spare word (`shaders/mrms.wgsl`), so those
//! sliders are free — no LUT re-bake.

use crate::render::FieldLayer;
use crate::settings::Settings;

/// One imported GIS layer's row: what was read from its file.
pub(crate) struct GisRow {
    pub id: u64,
    pub features: usize,
    /// Why its file could not be read, when it could not.
    pub error: Option<String>,
}

/// What the Layer Manager shows about the imported GIS layers beyond their settings.
pub(crate) struct Imported<'a> {
    /// Every layer, in `settings.gis_layers` order.
    pub rows: &'a [GisRow],
    /// Every attribute name in the edited layer's file: the choices for labels, colours and
    /// times.
    pub keys: &'a [String],
    /// The colour-by legend, when one is on.
    pub legend: Option<&'a crate::gis_import::Legend>,
    /// With a time mapping or a filter: how many features are shown, of all.
    pub time_count: Option<(usize, usize)>,
    /// Why the edited layer's filter does not parse (the previous one stays in use).
    pub filter_error: Option<String>,
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

/// What the window asks of the app.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Anything changed: the overlays are reassembled.
    pub changed: bool,
    /// Remove this imported layer.
    pub remove: Option<u64>,
    /// Frame the map on this imported layer.
    pub zoom: Option<u64>,
    /// Write this imported layer's shown features out as GeoJSON.
    pub export: Option<u64>,
    /// Open this imported layer's feature table.
    pub table: Option<u64>,
}

/// Show the window. `active` is the field layers currently painting, with their display names
/// (only those get a slider); `selected` is the imported layer being edited.
pub(crate) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    active: &[(FieldLayer, String)],
    imported: &Imported,
    selected: &mut Option<u64>,
    drawer: &mut crate::ui::drawer::Drawer,
) -> Outcome {
    let mut out = Outcome::default();
    if !*open {
        return out;
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
        return out;
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
        if !settings.gis_layers.is_empty() {
            gis_layers(ui, settings, imported, selected, &mut out);
            changed |= out.changed;
            ui.separator();
        }
        if settings.placefiles.is_empty() {
            if settings.gis_layers.is_empty() && active.is_empty() {
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
    out.changed |= changed;
    out
}

/// The imported GIS layers (ROADMAP_PARITY M4.1): one row each — visibility, name, what was read
/// or why not, order, zoom-to and remove — the groups' switches, and the editor for the selected
/// layer's style, labels, colouring, time mapping, side and group.
fn gis_layers(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    imported: &Imported,
    selected: &mut Option<u64>,
    out: &mut Outcome,
) {
    let mut changed = false;
    ui.label(egui::RichText::new("Imported GIS layers").strong());
    ui.weak("Top of the list paints first (underneath).");
    let n = settings.gis_layers.len();
    let mut shift: Option<(u64, isize)> = None;
    for i in 0..n {
        let id = settings.gis_layers[i].id;
        let row = imported.rows.iter().find(|r| r.id == id);
        ui.horizontal(|ui| {
            let layer = &mut settings.gis_layers[i];
            changed |= ui
                .checkbox(&mut layer.visible, "")
                .on_hover_text("Show this layer")
                .changed();
            let mut name = egui::RichText::new(&layer.name);
            if *selected == Some(id) {
                name = name.strong();
            }
            if ui
                .selectable_label(*selected == Some(id), name)
                .on_hover_text(&layer.source)
                .clicked()
            {
                *selected = Some(id);
            }
            match row {
                Some(GisRow { error: Some(e), .. }) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 60), "missing")
                        .on_hover_text(format!(
                            "{e}\nRe-import the file to restore this layer with its settings, or \
                             remove it."
                        ));
                }
                Some(r) => {
                    ui.weak(r.features.to_string())
                        .on_hover_text("Shapes, lines and points read from the file");
                }
                None => {}
            }
            if let Some(g) = &layer.group {
                ui.weak(format!("· {g}"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("✕")
                    .on_hover_text("Remove this layer")
                    .clicked()
                {
                    out.remove = Some(id);
                }
                if ui
                    .small_button("⌖")
                    .on_hover_text("Zoom to this layer")
                    .clicked()
                {
                    out.zoom = Some(id);
                }
                if ui
                    .small_button("\u{25a6}")
                    .on_hover_text("The features this layer shows, as a table")
                    .clicked()
                {
                    out.table = Some(id);
                }
                if ui
                    .small_button("⤓")
                    .on_hover_text(
                        "Export the features this layer shows (filter and time applied) as GeoJSON",
                    )
                    .clicked()
                {
                    out.export = Some(id);
                }
                if ui
                    .add_enabled(i + 1 < n, egui::Button::new("▼"))
                    .on_hover_text("Paint later (on top)")
                    .clicked()
                {
                    shift = Some((id, 1));
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("▲"))
                    .on_hover_text("Paint earlier (underneath)")
                    .clicked()
                {
                    shift = Some((id, -1));
                }
            });
        });
    }
    if let Some((id, d)) = shift {
        settings.move_gis_layer(id, d);
        changed = true;
    }
    // Groups in use get a switch; hiding one keeps its layers' own checkboxes as they are.
    let used: Vec<String> = {
        let mut g: Vec<String> = settings
            .gis_layers
            .iter()
            .filter_map(|l| l.group.clone())
            .collect();
        g.sort();
        g.dedup();
        g
    };
    for name in &used {
        if !settings.gis_groups.iter().any(|g| &g.name == name) {
            settings.gis_groups.push(crate::settings::GisGroup {
                name: name.clone(),
                visible: true,
            });
        }
    }
    settings.gis_groups.retain(|g| used.contains(&g.name));
    if !settings.gis_groups.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label("Groups");
            for g in &mut settings.gis_groups {
                changed |= ui
                    .checkbox(&mut g.visible, &g.name)
                    .on_hover_text("Show this group's layers (each keeps its own switch)")
                    .changed();
            }
        });
    }
    let Some(i) = selected.and_then(|id| settings.gis_layers.iter().position(|l| l.id == id))
    else {
        out.changed |= changed;
        return;
    };
    ui.add_space(4.0);
    let groups: Vec<String> = settings.gis_groups.iter().map(|g| g.name.clone()).collect();
    let layer = &mut settings.gis_layers[i];
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut layer.name);
    });
    ui.horizontal(|ui| {
        ui.label("Group")
            .on_hover_text("Type a name to start a group");
        let mut text = layer.group.clone().unwrap_or_default();
        let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
        if edit.changed() {
            let t = text.trim();
            layer.group = (!t.is_empty()).then(|| t.to_string());
            changed = true;
        }
        egui::ComboBox::from_id_salt("gis_layer_group")
            .selected_text("…")
            .width(24.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(layer.group.is_none(), "None").clicked() {
                    layer.group = None;
                    changed = true;
                }
                for g in &groups {
                    if ui
                        .selectable_label(layer.group.as_deref() == Some(g.as_str()), g)
                        .clicked()
                    {
                        layer.group = Some(g.clone());
                        changed = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Color");
        changed |= ui
            .color_edit_button_srgb(&mut layer.style.color)
            .on_hover_text("Color for this layer's polygons, lines, and points")
            .changed();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Reset style")
                .on_hover_text("Restore the neutral-blue imported-layer style")
                .clicked()
            {
                layer.style = Default::default();
                changed = true;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Outline");
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.stroke_width, 0.5..=8.0)
                    .suffix(" px")
                    .max_decimals(1),
            )
            .on_hover_text("Width for this layer's polygon edges, lines, and point symbols")
            .changed();
        egui::ComboBox::from_id_salt("imported_gis_dash")
            .selected_text(layer.style.dash.label())
            .width(80.0)
            .show_ui(ui, |ui| {
                for d in crate::settings::LineDash::ALL {
                    changed |= ui
                        .selectable_value(&mut layer.style.dash, d, d.label())
                        .changed();
                }
            })
            .response
            .on_hover_text("How lines and polygon outlines are drawn");
    });
    ui.horizontal(|ui| {
        let mut own = layer.style.fill_color.is_some();
        if ui
            .checkbox(&mut own, "Own fill")
            .on_hover_text(
                "Fill polygons in a colour of their own; with Color by, the attribute then \
                 colours the fill and the outline keeps the layer colour",
            )
            .changed()
        {
            layer.style.fill_color = own.then_some(layer.style.color);
            changed = true;
        }
        if let Some(fill) = layer.style.fill_color.as_mut() {
            changed |= ui.color_edit_button_srgb(fill).changed();
        }
        ui.label("Fill");
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.fill_opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .on_hover_text("How much of the polygon fill shows; 0% draws outlines only")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Points");
        egui::ComboBox::from_id_salt("imported_gis_symbol")
            .selected_text(layer.style.symbol.label())
            .width(80.0)
            .show_ui(ui, |ui| {
                for sym in crate::settings::PointSymbol::ALL {
                    changed |= ui
                        .selectable_value(&mut layer.style.symbol, sym, sym.label())
                        .changed();
                }
            });
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.point_size, 0.0..=16.0)
                    .step_by(0.5)
                    .custom_formatter(|v, _| {
                        if v <= 0.0 {
                            "auto".to_string()
                        } else {
                            format!("{v:.1} px")
                        }
                    }),
            )
            .on_hover_text("Point symbol radius; auto follows the outline width")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Opacity");
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.opacity, 0.05..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .on_hover_text(format!("Opacity {:.0}%", layer.style.opacity * 100.0))
            .changed();
    });
    let keys = imported.keys;
    ui.horizontal(|ui| {
        ui.label("Label")
            .on_hover_text("Label each feature with this attribute's value");
        changed |= attribute_combo(ui, "imported_gis_label", "None", keys, &mut layer.label);
    });
    ui.horizontal(|ui| {
        ui.label("Label text").on_hover_text(
            "Build labels from several attributes, e.g. {NAME} ({POP}); used instead of Label \
             when set. A feature with none of them is not labelled.",
        );
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut layer.label_template)
                    .desired_width(f32::INFINITY)
                    .hint_text("{NAME}"),
            )
            .changed();
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
            &mut layer.color_by,
        );
    });
    if layer.color_by.is_some() {
        if let Some(legend) = imported.legend {
            color_legend(ui, legend);
        }
    }
    ui.horizontal(|ui| {
        ui.label("Valid from").on_hover_text(
            "Show each feature only from the time in this attribute, following the timeline",
        );
        changed |= attribute_combo(
            ui,
            "imported_gis_time_start",
            "Always",
            keys,
            &mut layer.time_start,
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
            &mut layer.time_end,
        );
    });
    ui.horizontal(|ui| {
        ui.label("Filter").on_hover_text(
            "Show only features this is true for, e.g. POP > 1000 and TYPE = \"school\". \
             Compare with =, !=, <, <=, >, >= or contains; combine with and, or, not; \
             NAME is missing. A feature without the attribute is not shown; names with spaces \
             go in `backticks`.",
        );
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut layer.filter)
                    .desired_width(f32::INFINITY)
                    .hint_text("every feature"),
            )
            .changed();
    });
    if let Some(e) = &imported.filter_error {
        ui.colored_label(
            egui::Color32::from_rgb(230, 130, 130),
            format!("Filter not applied: {e} (the previous one is still in use)"),
        );
    }
    if let Some((shown, total)) = imported.time_count {
        ui.weak(format!(
            "{shown} of {total} features shown (valid at the view's time and passing the filter)"
        ));
    }
    changed |= ui
        .checkbox(&mut layer.targets, "Impact targets")
        .on_hover_text(
            "List a storm's arrival and closest approach at this layer's points, and when its \
             path enters each area, with the storm (named by the Label attribute)",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut layer.below,
            "Draw under warnings, watches and outlooks",
        )
        .on_hover_text(
            "Paint this layer's polygons beneath the official products instead of over them; \
             clicks prefer the official shape either way",
        )
        .changed();
    ui.horizontal(|ui| {
        ui.label("Show from zoom");
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.min_zoom, 0.0..=14.0)
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
                "Hide this layer when zoomed out past this (about 4 is the whole U.S., 7 a \
                 state, 10 a county)",
            )
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Show up to zoom");
        changed |= ui
            .add(
                egui::Slider::new(&mut layer.style.max_zoom, 0.0..=16.0)
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
                "Hide this layer when zoomed in past this, e.g. a state outline giving way to \
                 counties",
            )
            .changed();
    });
    ui.weak(
        "Color, outline and opacity apply to every geometry; Color by recolors each feature. \
         Styles never change the imported file.",
    );
    out.changed |= changed;
}

/// The imported layer's colour key: a gradient bar with its range, or a swatch per category.
fn color_legend(ui: &mut egui::Ui, legend: &crate::gis_import::Legend) {
    use crate::gis_import::Legend;
    let swatch = |ui: &mut egui::Ui, [r, g, b]: [u8; 3]| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(r, g, b));
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
