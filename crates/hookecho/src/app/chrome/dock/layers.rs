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

#[derive(Debug, PartialEq, Eq)]
enum SearchSubmit {
    Action(crate::app::PaletteAction),
    Place(String),
}

/// A shape as well as a color for every feed state: the compact tree stays readable when color
/// cannot distinguish its status dots. The full word remains in the hover and accessible name.
fn health_glyph(state: HealthState) -> &'static str {
    match state {
        HealthState::Fresh => ph::CHECK_CIRCLE,
        HealthState::Fetching => ph::ARROWS_CLOCKWISE,
        HealthState::Delayed => ph::CLOCK,
        HealthState::Stale => ph::WARNING_CIRCLE,
        HealthState::Cached => ph::DATABASE,
        HealthState::Failed => ph::X_CIRCLE,
        HealthState::Waiting => ph::HOURGLASS,
    }
}

/// Enter uses the same visible search results as a click. A time command takes precedence;
/// otherwise it opens the first layer row, or offers the place lookup when nothing matches.
fn submit_search(
    entries: &[PaletteEntry],
    tab: DockTab,
    filter: LayerFilter,
    query: &str,
    favorites: &[String],
    day: chrono::NaiveDate,
) -> Option<SearchSubmit> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if let Some(command) = crate::ui::layers_panel::command_entry(query, day) {
        return Some(SearchSubmit::Action(command.action));
    }
    if let Some(i) = group_entries(entries, tab, filter, query, favorites)
        .first()
        .and_then(|group| group.rows.first())
    {
        return Some(SearchSubmit::Action(entries[*i].action));
    }
    Some(SearchSubmit::Place(query.to_string()))
}

impl HookEchoApp {
    pub(super) fn dock_layers(&mut self, host: Host<'_>, ctx: &egui::Context) {
        if !self.dock.layers.open {
            return;
        }
        let t = self.ws_tokens();
        let entries = self.palette_entries();
        let active = entries.iter().filter(|e| e.on == Some(true)).count();
        let mut hit = None;
        let selected_day = self.views[self.active].timeline.date;
        let focus_search = std::mem::take(&mut self.dock.focus_search);
        let mut search_enter = false;
        let mut fly_to = None;
        let mut footer = None;
        let model_input = self.model_panel_input();
        let model_on = self.views[self.active].fields_on.clone();
        let model_tz = self.active_tz();
        let mut ui_actions = crate::ui::layer_options::UiActions::default();
        let place = self.dock.layers.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.layers.collapsed;
        let map_rect = self.chrome_rect;
        // A floating window has no panel to fill, so it gets a height of its own.
        let float_list_h = (map_rect.height() - 170.0).clamp(160.0, 480.0);
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_layers",
                place,
                width: LEFT_WIDTH,
                float_at: map_rect.left_top() + egui::vec2(12.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::STACK,
                    "Layers",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut self.dock.query)
                                .hint_text(format!(
                                    "{}  Search layers, sites, tools (Ctrl+K)",
                                    ph::MAGNIFYING_GLASS
                                ))
                                .desired_width(f32::INFINITY),
                        );
                        if focus_search {
                            search.request_focus();
                        }
                        search_enter = (search.has_focus() || search.lost_focus())
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
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
                let groups = group_entries(
                    &entries,
                    self.dock.tab,
                    self.dock.filter,
                    &self.dock.query,
                    &self.settings.favorite_layers,
                );
                // Typed commands ("time 21:30Z", "at now") share the search box.
                let command =
                    crate::ui::layers_panel::command_entry(&self.dock.query, selected_day);
                let footer_h = 40.0;
                let list_h = if floating {
                    float_list_h
                } else {
                    (ui.available_height() - footer_h).max(80.0)
                };
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
                        if plain && self.dock.tab == DockTab::Satellite {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    crate::ui::layer_options::qpe_window_control(
                                        ui,
                                        &model_on,
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
                        // A typed command answers first; a place name is the explicit last row.
                        if let Some(c) = &command {
                            if ws::button(ui, &t, &c.label, ui.available_width() - 20.0)
                                .on_hover_text(c.desc)
                                .clicked()
                            {
                                hit = Some(Hit::Row(c.action));
                            }
                            ui.add_space(4.0);
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
                let query = self.dock.query.trim();
                if !query.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let label = format!("{}  Fly to \u{201c}{query}\u{201d}", ph::MAP_PIN);
                        if ws::button(ui, &t, &label, ui.available_width() - 10.0)
                            .named("Look the place up and move the map there")
                            .clicked()
                        {
                            fly_to = Some(query.to_string());
                        }
                    });
                    ui.add_space(4.0);
                }
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
            },
        );
        // The model controls and the layer options both report through one actions struct.
        let from_panels = ui_actions.palette.take();
        self.apply_ui_actions(ui_actions, ctx);
        apply_header(header, &mut self.dock.layers);
        if search_enter && hit.is_none() {
            match submit_search(
                &entries,
                self.dock.tab,
                self.dock.filter,
                &self.dock.query,
                &self.settings.favorite_layers,
                selected_day,
            ) {
                Some(SearchSubmit::Action(action)) => hit = Some(Hit::Row(action)),
                Some(SearchSubmit::Place(place)) => fly_to = Some(place),
                None => {}
            }
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
        if let Some(place) = fly_to {
            self.start_place_search(place, ctx);
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
    let accessible_label = if let Some(health) = &e.health {
        let (word, _) = crate::ui::layers_panel::health_look(health.state());
        format!("{}; {} source: {word}", e.label, health.source)
    } else {
        e.label.clone()
    };
    let resp = if e.desc.is_empty() {
        resp.named_toggle(&accessible_label, on)
    } else {
        resp.named_toggle(&accessible_label, on)
            .on_hover_text(e.desc)
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
        let mark = egui::pos2(rect.right() - 40.0, y);
        p.text(
            mark,
            egui::Align2::CENTER_CENTER,
            health_glyph(h.state()),
            FontId::proportional(12.0),
            color,
        );
        let mark_rect = Rect::from_center_size(mark, egui::vec2(14.0, 16.0));
        ui.interact(
            mark_rect,
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

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::app::PaletteAction;

    fn entry(label: &str, category: &'static str, action: PaletteAction) -> PaletteEntry {
        PaletteEntry {
            label: label.to_string(),
            category,
            action,
            on: None,
            desc: "",
            common: false,
            key: None,
            health: None,
        }
    }

    #[test]
    fn enter_uses_the_visible_result_or_place_lookup() {
        let entries = [
            entry("Velocity", "Radar", PaletteAction::Reload),
            entry("KTLX", "Sites", PaletteAction::GoLive),
        ];
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        let submit =
            |query| submit_search(&entries, DockTab::Radar, LayerFilter::All, query, &[], day);
        assert_eq!(
            submit("KTLX"),
            Some(SearchSubmit::Action(PaletteAction::GoLive))
        );
        assert_eq!(
            submit("at now"),
            Some(SearchSubmit::Action(PaletteAction::GoLive))
        );
        assert_eq!(
            submit("Norman, Oklahoma"),
            Some(SearchSubmit::Place("Norman, Oklahoma".into()))
        );
        assert_eq!(submit("  "), None);
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;

    #[test]
    fn source_health_states_have_distinct_visible_shapes() {
        let states = [
            HealthState::Fresh,
            HealthState::Fetching,
            HealthState::Delayed,
            HealthState::Stale,
            HealthState::Cached,
            HealthState::Failed,
            HealthState::Waiting,
        ];
        let glyphs: std::collections::HashSet<_> = states.map(health_glyph).into_iter().collect();
        assert_eq!(glyphs.len(), states.len());
    }
}
