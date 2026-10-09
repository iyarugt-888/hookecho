//! User-defined products (Phase C1): add/edit/remove GR2Analyst-style formula products.
//!
//! This window manages the saved list (`Settings::udp_products`) and which products the active pane
//! shows on the map — a gate formula in place of the moment (`MapView::user_product`), a column
//! formula as a 2D field over it (`MapView::column_product`); it does not evaluate anything itself. A saved
//! product's live value at whatever point is clicked shows up in the gate inspector
//! (`ui::gate_inspector`), which reads this same list.

use crate::settings::Settings;
use wxdata::udp::{Input, ProductDef};

#[derive(Default)]
pub struct UdpWindow {
    pub open: bool,
    new_name: String,
    new_units: String,
    new_expression: String,
    show_reference: bool,
    /// What the last import or export did, product by product.
    pub report: Vec<String>,
}

impl UdpWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut Settings,
        drawer: &mut crate::ui::drawer::Drawer,
        // The active pane's gate product shown on the map, by name.
        on_map: &mut Option<String>,
        // The active pane's column product shown on the map, by name.
        column_on_map: &mut Option<String>,
        // Why the active pane's column product is not drawn, when it is selected and not drawn.
        column_status: Option<String>,
    ) {
        let mut open = self.open;
        let Some(window) = drawer.page_sized(
            ctx,
            "User-defined products",
            &mut open,
            false,
            560.0,
            egui::Window::new("User-defined products"),
        ) else {
            self.open = open;
            return;
        };
        window.show(ctx, |ui| {
            ui.label(
                "Combine a gate's own moments and geometry into a custom value — GR2Analyst's \
                 user-defined products. Show one on the map in place of the moment, draw it in 3D \
                 (\"User\" in the 3D controls), or click the map to read it at a gate. A column \
                 formula (max_vertical, max_layer, …) is drawn as a 2D field over the tilt.",
            );
            ui.add_space(4.0);
            if ui
                .link("Reference: inputs and functions")
                .on_hover_text("Click to show or hide the formula reference")
                .clicked()
            {
                self.show_reference = !self.show_reference;
            }
            if self.show_reference {
                reference(ui);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !settings.udp_products.is_empty(),
                        egui::Button::new("Export…"),
                    )
                    .on_hover_text(
                        "Save these products as a portable file, each with a stable ID and the \
                         inputs, environment and height convention it needs",
                    )
                    .clicked()
                {
                    wxdata::udp_file::ensure_ids(&mut settings.udp_products);
                    let text = wxdata::udp_file::export(&settings.udp_products);
                    self.report = vec![match crate::dialog::save_bytes(
                        "hookecho-products.json",
                        "json",
                        text.as_bytes(),
                    ) {
                        crate::dialog::Saved::Where(w) => format!(
                            "Exported {} product{} to {w}",
                            settings.udp_products.len(),
                            if settings.udp_products.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ),
                        crate::dialog::Saved::Failed(e) => format!("Export failed: {e}"),
                        crate::dialog::Saved::Cancelled => String::new(),
                    }];
                    self.report.retain(|l| !l.is_empty());
                }
                if ui
                    .button("Import…")
                    .on_hover_text(
                        "Add products from a file; one already here (same ID) is updated, and \
                         each product is checked before it is added",
                    )
                    .clicked()
                {
                    crate::dialog::request_open(crate::dialog::ImportKind::UdpProducts, "");
                }
            });
            for line in &self.report {
                ui.weak(line);
            }
            ui.separator();

            let mut remove: Option<usize> = None;
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for i in 0..settings.udp_products.len() {
                        ui.push_id(i, |ui| {
                            row(ui, settings, i, &mut remove, on_map, column_on_map);
                        });
                        ui.separator();
                    }
                });
            if let Some(i) = remove {
                let gone = settings.udp_products.remove(i);
                if on_map.as_deref() == Some(gone.name.as_str()) {
                    *on_map = None;
                }
                if column_on_map.as_deref() == Some(gone.name.as_str()) {
                    *column_on_map = None;
                }
            }

            if let Some(why) = &column_status {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 190, 110),
                    format!("Column product: {why}"),
                );
            }
            ui.add_space(6.0);
            ui.strong("Add a product");
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_name)
                        .desired_width(140.0)
                        .hint_text("Hail signature"),
                );
                ui.label("Units:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_units)
                        .desired_width(60.0)
                        .hint_text("dBZ"),
                );
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.new_expression)
                    .desired_width(f32::INFINITY)
                    .hint_text("REF > 55 && ZDR < 1 ? REF : 0"),
            );
            let name = self.new_name.trim().to_string();
            let expr = self.new_expression.trim().to_string();
            let parsed = wxdata::udp::parse(&expr);
            if let Err(e) = &parsed {
                if !expr.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(230, 130, 130), e.to_string());
                }
            }
            if let Ok(parsed) = &parsed {
                unit_notes(ui, parsed, &self.new_units);
            }
            let supported = parsed
                .as_ref()
                .is_ok_and(|e| e.column_depth() <= wxdata::udp_column::MAX_COLUMN_DEPTH);
            if parsed
                .as_ref()
                .is_ok_and(|e| e.column_depth() > wxdata::udp_column::MAX_COLUMN_DEPTH)
            {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 130, 130),
                    "Column reductions may nest at most twice",
                );
            }
            let valid = !name.is_empty() && supported;
            if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
                let mut def = ProductDef {
                    id: String::new(),
                    name,
                    units: self.new_units.trim().to_string(),
                    expression: expr,
                    range: None,
                    palette: None,
                };
                // Its ID is fixed now, so renaming or editing it later keeps it the same product.
                def.id = wxdata::udp_file::derive_id(&def);
                settings.udp_products.push(def);
                self.new_name.clear();
                self.new_units.clear();
                self.new_expression.clear();
            }
        });
        self.open = open;
    }
}

/// Unit and datum notes for a formula (1008.md C4), in amber: advice, since a weighted index may
/// mean what it does, unlike a formula that does not parse.
fn unit_notes(ui: &mut egui::Ui, expr: &wxdata::udp::Expr, units: &str) {
    let amber = egui::Color32::from_rgb(230, 180, 90);
    let label = wxdata::udp::label_note(units, expr.result_quantity());
    for note in expr.unit_diagnostics().into_iter().chain(label) {
        ui.add(
            egui::Label::new(egui::RichText::new(format!("\u{26a0} {note}")).color(amber)).wrap(),
        );
    }
}

/// One saved product's editable row: name/units/expression fields plus a live compile-error
/// readout, so a typo shows up here rather than only as a silent "—" in the gate inspector later.
fn row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    i: usize,
    remove: &mut Option<usize>,
    on_map: &mut Option<String>,
    column_on_map: &mut Option<String>,
) {
    let def = &mut settings.udp_products[i];
    let old_name = def.name.clone();
    ui.horizontal(|ui| {
        if ui.button("\u{2716}").on_hover_text("Remove").clicked() {
            *remove = Some(i);
        }
        ui.add(egui::TextEdit::singleline(&mut def.name).desired_width(140.0));
        ui.label("units:");
        ui.add(egui::TextEdit::singleline(&mut def.units).desired_width(50.0));
    });
    // A rename keeps it on the map.
    if def.name != old_name && on_map.as_deref() == Some(old_name.as_str()) {
        *on_map = Some(def.name.clone());
    }
    if def.name != old_name && column_on_map.as_deref() == Some(old_name.as_str()) {
        *column_on_map = Some(def.name.clone());
    }
    ui.add(egui::TextEdit::singleline(&mut def.expression).desired_width(f32::INFINITY));
    let compiled = def.compile();
    if let Err(e) = &compiled {
        ui.colored_label(egui::Color32::from_rgb(230, 130, 130), e.to_string());
    }
    if let Ok(expr) = &compiled {
        unit_notes(ui, expr, &def.units);
    }
    let per_gate = compiled.as_ref().is_ok_and(|e| !e.uses_column());
    let per_column = compiled.as_ref().is_ok_and(|e| e.uses_column());
    let supported = compiled
        .as_ref()
        .is_ok_and(|e| e.column_depth() <= wxdata::udp_column::MAX_COLUMN_DEPTH);
    if per_column && !supported {
        ui.colored_label(
            egui::Color32::from_rgb(230, 130, 130),
            "Column reductions may nest at most twice",
        );
    }
    ui.horizontal(|ui| {
        if per_column {
            let shown = column_on_map.as_deref() == Some(def.name.as_str());
            let r = ui
                .add_enabled(supported, egui::Button::selectable(shown, "Show on map"))
                .on_hover_text(
                    "Draw it as a 2D field over the map: one value per ground point, from every \
                     tilt of this volume over it",
                );
            if r.clicked() {
                *column_on_map = (!shown).then(|| def.name.clone());
            }
        } else {
            let shown = on_map.as_deref() == Some(def.name.as_str());
            let r = ui
                .add_enabled(per_gate, egui::Button::selectable(shown, "Show on map"))
                .on_hover_text("Draw it on the map in place of the moment, on the shown tilt")
                .on_disabled_hover_text("Fix the formula first");
            if r.clicked() {
                *on_map = (!shown).then(|| def.name.clone());
            }
        }
        let mut fixed = def.range.is_some();
        if ui
            .checkbox(&mut fixed, "Fixed range")
            .on_hover_text(
                "Colour it over a range you set. Off: over the range it comes out in (its 2nd to \
                 98th percentile)",
            )
            .changed()
        {
            def.range = fixed.then_some((0.0, 100.0));
        }
        if let Some((lo, hi)) = &mut def.range {
            ui.add(egui::DragValue::new(lo).speed(0.5).prefix("from "));
            ui.add(egui::DragValue::new(hi).speed(0.5).prefix("to "));
            if *hi <= *lo {
                *hi = *lo + 1.0;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Colours:");
        let shown = def.palette.clone().unwrap_or_else(|| "Ramp".into());
        egui::ComboBox::from_id_salt(("udp_palette", i))
            .selected_text(match def.palette.as_deref() {
                Some(code) => format!("{code} colour table"),
                None => shown,
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut def.palette, None, "Ramp")
                    .on_hover_text("A plain ramp, blue to magenta, across its range");
                for m in wxdata::level2::Moment::ALL {
                    let code = m.short_name().to_string();
                    ui.selectable_value(
                        &mut def.palette,
                        Some(code.clone()),
                        format!("{code} colour table"),
                    )
                    .on_hover_text(format!(
                        "Read in {code}'s colours and units ({}): drawn over that table's range \
                         unless a fixed range is set",
                        m.units()
                    ));
                }
            });
    });
}

fn reference(ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label("Inputs (case-insensitive):");
        ui.horizontal_wrapped(|ui| {
            for input in Input::ALL {
                ui.label(egui::RichText::new(input.name()).monospace());
            }
        });
        ui.add_space(4.0);
        ui.label(
            "Functions: min(a,b)  max(a,b)  mean(a,b,...) [2–8 values]  clamp(x,lo,hi)  abs(x)",
        );
        ui.weak(
            "BEAM_HEIGHT_M is above the radar; BEAM_ALTITUDE_M, FREEZING_LEVEL_M, \
             MINUS10C_HEIGHT_M through MINUS40C_HEIGHT_M are above sea level when known — compare the \
             isotherms against BEAM_ALTITUDE_M, not BEAM_HEIGHT_M. VEL is dealiased.",
        );
        ui.weak("Archived isotherms use recorded sounding HGHT (geopotential metres MSL). −30/−40 °C are unavailable from the live HRRR source. Missing levels are never estimated from another isotherm.");
        ui.label(
            "Functions: min max mean(2-8) clamp abs   max_vertical(e[,cond]) min_vertical(e[,cond]) \
             mean_vertical(e[,cond]) max_height(e[,cond]) min_height(e[,cond]) \
             max_layer(e,lo,hi[,cond]) min_layer mean_layer \
             first_height_above(e,t) last_height_above(e,t) count_above(e,t) \
             count_vertical(cond) fraction_vertical(cond) \
             first_crossing_height(e,t) last_crossing_height(e,t) integral_layer(e,lo,hi)",
        );
        ui.weak(
            "Layer bounds and returned heights are metres above the antenna (ARL), not terrain \
             AGL or MSL. Heights are sampled beams, not interpolated crossings; extrema ties \
             choose the lowest height. Crossing functions interpolate adjacent recorded beams; \
             integral_layer returns value × metres and requires the whole interval to be \
             bracketed with no missing samples. Fractions use recorded conditions as the \
             denominator, excluding missing samples. Reductions may nest twice, with a bounded map cost.",
        );
        ui.label("Operators: + - * /   < <= > >= == !=   && || !   cond ? a : b");
        ui.add_space(4.0);
        ui.weak(
            "A formula referencing a moment that has no value at a gate (below threshold, \
             range-folded, or not carried there) evaluates to nothing there, same as the moment \
             itself. The isotherm heights come from the live HRRR analysis while following the \
             feed and from that day's observed sounding on an archived volume — never one for \
             the other; a formula needing a height neither has is not drawn. A column formula's \
             bare inputs (outside a vertical function) read the lowest level over the point.",
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(name: &str, units: &str, expression: &str) -> ProductDef {
        ProductDef {
            id: String::new(),
            name: name.into(),
            units: units.into(),
            expression: expression.into(),
            range: None,
            palette: None,
        }
    }

    /// Saved products with a unit mix, a datum mix and a units label that names another
    /// quantity, and one that does not parse, as the editor shows them (1008.md C4).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the product editor's unit notes"]
    fn gpu_unit_notes_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the editor");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m3.2");
        std::fs::create_dir_all(&destination).unwrap();
        let mut settings = Settings {
            udp_products: vec![
                product("Shear-ish", "dBZ", "REF + ZDR"),
                product(
                    "Hail above 0 C",
                    "dBZ",
                    "BEAM_HEIGHT_M >= FREEZING_LEVEL_M ? REF : 0/0",
                ),
                product("Wind core", "dBZ", "abs(VEL)"),
                product("Typo", "dBZ", "REF >= 50 && ZDRR < 1"),
            ],
            ..Default::default()
        };
        gpu.save(&destination.join("unit-notes.png"), 520, 520, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(500.0);
                for i in 0..settings.udp_products.len() {
                    row(ui, &mut settings, i, &mut None, &mut None, &mut None);
                    ui.separator();
                }
            });
        })
        .unwrap();
    }
}
