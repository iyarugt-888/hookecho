//! User-defined products (Phase C1): add/edit/remove GR2Analyst-style formula products.
//!
//! This window only manages the saved list (`Settings::udp_products`); it does not evaluate
//! anything itself. A saved product's live value at whatever point is clicked shows up in the
//! gate inspector (`ui::gate_inspector`), which reads this same list.

use crate::settings::Settings;
use wxdata::udp::{Input, ProductDef};

#[derive(Default)]
pub struct UdpWindow {
    pub open: bool,
    new_name: String,
    new_units: String,
    new_expression: String,
    show_reference: bool,
}

impl UdpWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut Settings,
        drawer: &mut crate::ui::drawer::Drawer,
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
                 user-defined products. See its live result by clicking the radar map (gate \
                 inspector).",
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
            ui.separator();

            let mut remove: Option<usize> = None;
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for i in 0..settings.udp_products.len() {
                    ui.push_id(i, |ui| {
                        row(ui, settings, i, &mut remove);
                    });
                    ui.separator();
                }
            });
            if let Some(i) = remove {
                settings.udp_products.remove(i);
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
            let valid = !name.is_empty() && parsed.is_ok();
            if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
                settings.udp_products.push(ProductDef {
                    name,
                    units: self.new_units.trim().to_string(),
                    expression: expr,
                });
                self.new_name.clear();
                self.new_units.clear();
                self.new_expression.clear();
            }
        });
        self.open = open;
    }
}

/// One saved product's editable row: name/units/expression fields plus a live compile-error
/// readout, so a typo shows up here rather than only as a silent "—" in the gate inspector later.
fn row(ui: &mut egui::Ui, settings: &mut Settings, i: usize, remove: &mut Option<usize>) {
    let def = &mut settings.udp_products[i];
    ui.horizontal(|ui| {
        if ui.button("\u{2716}").on_hover_text("Remove").clicked() {
            *remove = Some(i);
        }
        ui.add(egui::TextEdit::singleline(&mut def.name).desired_width(140.0));
        ui.label("units:");
        ui.add(egui::TextEdit::singleline(&mut def.units).desired_width(50.0));
    });
    ui.add(egui::TextEdit::singleline(&mut def.expression).desired_width(f32::INFINITY));
    if let Err(e) = def.compile() {
        ui.colored_label(egui::Color32::from_rgb(230, 130, 130), e.to_string());
    }
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
        ui.label("Functions: min(a,b)  max(a,b)  clamp(x,lo,hi)  abs(x)");
        ui.label("Operators: + - * /   < <= > >= == !=   && || !   cond ? a : b");
        ui.add_space(4.0);
        ui.weak(
            "A formula referencing a moment that has no value at a gate (below threshold, \
             range-folded, or not carried there) evaluates to nothing there, same as the moment \
             itself. Vertical/layer aggregates (max over a column, freezing-level heights) \
             aren't available yet — this evaluates one gate at a time.",
        );
    });
}
