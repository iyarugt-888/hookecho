//! The Layers panel: search, the All / Active / Favorites filter, and the command registry as a
//! tree of categories. The Models and Analysis tabs draw their controls above the tree.

use super::*;
use crate::ui::a11y::Named as _;
use egui::{FontId, Rect, Sense, Stroke};
use egui_phosphor::regular as ph;

/// One tree row's height: tight enough to see a category at a glance, tall enough to hit.
const ROW_H: f32 = 26.0;

/// What a click in the tree asked for.
enum Hit {
    Row(crate::app::PaletteAction),
    Star(&'static str),
}

impl HookEchoApp {
    pub(super) fn dock_left(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if !self.dock.left_open {
            return;
        }
        let t = self.ws_tokens();
        let entries = self.palette_entries();
        let active = entries.iter().filter(|e| e.on == Some(true)).count();
        let groups = group_entries(
            &entries,
            self.dock.tab,
            self.dock.filter,
            &self.dock.query,
            &self.settings.favorite_layers,
        );
        let mut hit = None;
        let mut close = false;
        let mut footer = None;
        let model_input = self.model_panel_input();
        let model_on = self.views[self.active].fields_on.clone();
        let model_tz = self.active_tz();
        let mut ui_actions = crate::ui::layer_options::UiActions::default();
        egui::Panel::left("dock_layers")
            .exact_size(LEFT_WIDTH)
            .resizable(false)
            .frame(ws::panel_frame(&t))
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                if ws::panel_header(ui, &t, ph::STACK, "Layers", None) == ws::HeaderAction::Close {
                    close = true;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dock.query)
                                .hint_text(format!("{}  Search layers", ph::MAGNIFYING_GLASS))
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(4.0);
                        let active_label = format!("Active ({active})");
                        let labels = ["All", active_label.as_str(), "Favorites"];
                        let selected = match self.dock.filter {
                            LayerFilter::All => 0,
                            LayerFilter::Active => 1,
                            LayerFilter::Favorites => 2,
                        };
                        if let Some(i) = ws::segmented(ui, &t, &labels, selected) {
                            self.dock.filter = [
                                LayerFilter::All,
                                LayerFilter::Active,
                                LayerFilter::Favorites,
                            ][i];
                        }
                    });
                let footer_h = 40.0;
                let list_h = (ui.available_height() - footer_h).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // The tab's own controls come first while the tree is showing that tab;
                        // a search or a filter is about layers, so they step aside for it.
                        let plain =
                            self.dock.query.is_empty() && self.dock.filter == LayerFilter::All;
                        if plain && self.dock.tab == DockTab::Models {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    crate::ui::model_panel::show(
                                        ui,
                                        &model_input,
                                        &model_on,
                                        model_tz,
                                        &mut self.env_cape_ml,
                                        &mut self.env_srh_km,
                                        &mut self.fields,
                                        &mut ui_actions,
                                    );
                                });
                        }
                        if plain && self.dock.tab == DockTab::Analysis {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    ui.label(ws::text(
                                        "Settings of the layers that are on",
                                        11.5,
                                        t.text_dim,
                                    ));
                                    self.layer_options_body(ui, &mut ui_actions);
                                });
                        }
                        if groups.is_empty() {
                            let why = match self.dock.filter {
                                LayerFilter::Favorites if self.dock.query.is_empty() => {
                                    "No favourites yet: star a layer to keep it here."
                                }
                                LayerFilter::Active if self.dock.query.is_empty() => {
                                    "Nothing is switched on."
                                }
                                _ => "Nothing matches.",
                            };
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.label(ws::text(why, 12.0, t.text_dim));
                            });
                        }
                        // While searching or filtering, every category left standing has
                        // something in it the user asked for, so it opens regardless of the
                        // collapsed state it was left in.
                        let force_open = !plain;
                        for g in &groups {
                            if let Some(h) = group(
                                ui,
                                &t,
                                g,
                                &entries,
                                force_open,
                                &self.settings.favorite_layers,
                            ) {
                                hit = Some(h);
                            }
                        }
                    });
                // The footer: the ways to add a layer that is not in the registry yet.
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), footer_h),
                    Sense::hover(),
                );
                ui.painter().line_segment(
                    [rect.left_top(), rect.right_top()],
                    Stroke::new(1.0, t.line),
                );
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(egui::vec2(10.0, 6.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if ws::icon_button(&mut row, &t, ph::UPLOAD_SIMPLE, "Import\u{2026}", false)
                    .named("Import a GeoJSON or Shapefile as an overlay")
                    .clicked()
                {
                    footer = Some(crate::app::PaletteAction::ImportGis);
                }
                if ws::icon_button(
                    &mut row,
                    &t,
                    ph::SLIDERS_HORIZONTAL,
                    "Manage\u{2026}",
                    false,
                )
                .named("Order, group and configure layers")
                .clicked()
                {
                    footer = Some(crate::app::PaletteAction::OpenWindow(
                        AppWindow::LayerManager,
                    ));
                }
            });
        // The model controls and the layer options both report through one actions struct.
        let from_panels = ui_actions.palette.take();
        self.apply_ui_actions(ui_actions, ctx);
        if close {
            self.dock.left_open = false;
        }
        match hit {
            Some(Hit::Row(a)) => self.apply_palette(a, ctx),
            Some(Hit::Star(slug)) => {
                crate::ui::layers_panel::toggle_favorite(&mut self.settings.favorite_layers, slug);
            }
            None => {}
        }
        for a in [from_panels, footer].into_iter().flatten() {
            self.apply_palette(a, ctx);
        }
    }
}

/// One category: a header row that folds it, then its rows.
fn group(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    g: &Group,
    entries: &[PaletteEntry],
    force_open: bool,
    favorites: &[String],
) -> Option<Hit> {
    let id = ui.make_persistent_id(("dock_group", g.category));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        matches!(g.category, "Radar" | "Models" | "National" | "Severe"),
    );
    if force_open {
        state.set_open(true);
    }
    let tint = category_color(g.category);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H + 2.0),
        Sense::click(),
    );
    let name = crate::ui::layers_panel::category_name(g.category);
    let resp = resp.named_toggle(name, state.is_open());
    if resp.clicked() {
        state.toggle(ui);
    }
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0.0, t.panel_hi);
    }
    let y = rect.center().y;
    p.text(
        egui::pos2(rect.left() + 12.0, y),
        egui::Align2::LEFT_CENTER,
        if state.is_open() {
            ph::CARET_DOWN
        } else {
            ph::CARET_RIGHT
        },
        FontId::proportional(11.0),
        t.text_dim,
    );
    p.text(
        egui::pos2(rect.left() + 28.0, y),
        egui::Align2::LEFT_CENTER,
        crate::ui::layers_panel::category_glyph(g.category),
        FontId::proportional(14.0),
        tint,
    );
    p.text(
        egui::pos2(rect.left() + 48.0, y),
        egui::Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        t.text,
    );
    p.text(
        egui::pos2(rect.right() - 12.0, y),
        egui::Align2::RIGHT_CENTER,
        format!("{}/{}", g.on, g.rows.len()),
        FontId::proportional(11.5),
        if g.on > 0 { t.accent } else { t.text_faint },
    );
    let mut hit = None;
    state.show_body_unindented(ui, |ui| {
        for &i in &g.rows {
            if let Some(h) = row(ui, t, &entries[i], tint, favorites) {
                hit = Some(h);
            }
        }
    });
    hit
}

/// One layer: a checkbox (or, for a one-shot action, nothing to tick), the layer's glyph in its
/// category's tint, the name, its feed's health, and a star for the layers that can be starred.
fn row(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    e: &PaletteEntry,
    tint: egui::Color32,
    favorites: &[String],
) -> Option<Hit> {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
    let on = e.on == Some(true);
    let slug = favorite_slug(e);
    let star_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 18.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    let star = slug.map(|s| {
        (
            s,
            ui.interact(star_rect, ui.id().with(("dock_star", s)), Sense::click()),
        )
    });
    let over_star = star.as_ref().is_some_and(|(_, r)| r.hovered());
    let resp = if e.desc.is_empty() {
        resp.named_toggle(&e.label, on)
    } else {
        resp.named_toggle(&e.label, on).on_hover_text(e.desc)
    };
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 0.0, t.accent_soft().gamma_multiply(0.5));
        // The selected-state edge: a wash alone is easy to miss on a dim panel.
        p.rect_filled(
            Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
            0.0,
            t.accent,
        );
    } else if resp.hovered() || over_star {
        p.rect_filled(rect, 0.0, t.panel_hi);
    }
    let y = rect.center().y;
    let mut x = rect.left() + 30.0;
    if let Some(is_on) = e.on {
        let bx = Rect::from_center_size(egui::pos2(x + 7.0, y), egui::vec2(14.0, 14.0));
        if is_on {
            p.rect_filled(bx, 3.0, t.accent);
            p.text(
                bx.center(),
                egui::Align2::CENTER_CENTER,
                ph::CHECK,
                FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        } else {
            p.rect(
                bx,
                3.0,
                t.field,
                Stroke::new(1.0, if resp.hovered() { t.accent } else { t.line }),
                egui::StrokeKind::Inside,
            );
        }
    }
    x += 22.0;
    p.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        crate::ui::layers_panel::glyph(e),
        FontId::proportional(14.0),
        if on { tint } else { tint.gamma_multiply(0.7) },
    );
    x += 22.0;
    // The name gets whatever the trailing marks leave, cut with an ellipsis rather than spilling
    // under the star.
    let right = rect.right() - 34.0 - if e.health.is_some() { 14.0 } else { 0.0 };
    let mut job = egui::text::LayoutJob::simple_singleline(
        e.label.clone(),
        FontId::proportional(12.5),
        if on { egui::Color32::WHITE } else { t.text },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - x).max(20.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x, y - galley.size().y / 2.0), galley, t.text);
    if let Some(h) = &e.health {
        let (word, color) = crate::ui::layers_panel::health_look(h.state());
        let dot = egui::pos2(rect.right() - 40.0, y);
        p.circle_filled(dot, 3.5, color);
        let dot_rect = Rect::from_center_size(dot, egui::vec2(12.0, 12.0));
        ui.interact(
            dot_rect,
            ui.id().with(("dock_health", &e.label)),
            Sense::hover(),
        )
        .on_hover_text(format!("{}: {word}", h.source));
    }
    let mut hit = None;
    if let Some((s, star_resp)) = star {
        let starred = favorites.iter().any(|f| f == s);
        // An unstarred star only shows while the row is under the pointer, so the list reads as
        // names, not as a column of hollow stars.
        if starred || resp.hovered() || star_resp.hovered() {
            ui.painter().text(
                star_rect.center(),
                egui::Align2::CENTER_CENTER,
                ph::STAR,
                FontId::proportional(13.0),
                if starred {
                    t.warn
                } else if star_resp.hovered() {
                    t.text
                } else {
                    t.text_faint
                },
            );
        }
        let star_resp = star_resp.named_toggle(
            if starred {
                "Remove from favourites"
            } else {
                "Add to favourites"
            },
            starred,
        );
        if star_resp.clicked() {
            hit = Some(Hit::Star(s));
        }
    }
    if hit.is_none() && resp.clicked() {
        hit = Some(Hit::Row(e.action));
    }
    hit
}
