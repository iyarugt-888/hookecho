//! The dock's two top bars: the app bar (brand, workspace tabs, panel buttons, clock and radar
//! health) and the context toolbar (the radar controls you reach for while looking at the map).

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

impl HookEchoApp {
    pub(super) fn dock_app_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let t = self.ws_tokens();
        // The clock is only right if something repaints it; an idle map does not.
        ctx.request_repaint_after(std::time::Duration::from_secs(20));
        let tz = self.active_tz();
        let width = ctx.content_rect().width();
        let fit = app_bar_fit(width);
        let clock = if fit.date {
            crate::timefmt::fmt_date_clock(chrono::Utc::now(), tz)
        } else {
            crate::timefmt::fmt_clock(chrono::Utc::now(), tz, false)
        };
        let health = self.radar_health();
        let (state_word, state_color) = crate::ui::layers_panel::health_look(health.state());
        // Provider ingest lag: how far behind wall clock the newest live volume already was when
        // it landed. Only a real live arrival sets it, so it never reports a scrub as latency.
        let lag = self.views[self.active]
            .last_live_arrival
            .map(|(arrived, valid)| (arrived - valid).num_seconds().max(0));
        let keepout = if crate::os_decorated() {
            0.0
        } else {
            crate::ui::wsv3::WINDOW_BTN_KEEPOUT
        };
        let mut action = None;
        egui::Panel::top("dock_app_bar")
            .exact_size(ws::APP_BAR_H)
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .stroke(egui::Stroke::new(1.0, t.line_soft))
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(root, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(ws::text(ph::BROADCAST, 18.0, t.accent));
                    ui.add_space(4.0);
                    ui.label(ws::text("HookEcho", 14.5, egui::Color32::WHITE).strong());
                    ui.add_space(6.0);
                    if fit.subtitle {
                        ui.label(ws::text("Analyst Workstation", 12.0, t.text_faint));
                    }
                    ui.add_space(18.0);
                    for tab in DockTab::ALL {
                        let on = self.dock.left_open && self.dock.tab == tab;
                        if ws::tab(ui, &t, tab.label(), on, ws::APP_BAR_H)
                            .named_toggle(tab.label(), on)
                            .clicked()
                        {
                            // The open tab's own button folds the panel away; any other opens it
                            // on that tab.
                            if on {
                                self.dock.left_open = false;
                            } else {
                                self.dock.tab = tab;
                                self.dock.left_open = true;
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(keepout);
                        let lag_text = match lag {
                            Some(s) => format!("{state_word} \u{b7} {s} s lag"),
                            None => state_word.to_string(),
                        };
                        ui.label(ws::text(lag_text, 12.0, t.text_dim))
                            .on_hover_text(health.error.as_deref().unwrap_or(
                                "Radar feed health, and how far behind real time the newest \
                                 live volume already was when it arrived",
                            ));
                        ws::status_dot(ui, state_color, 4.0);
                        ui.add_space(8.0);
                        ui.label(ws::mono(clock, 12.0, t.text));
                        ui.label(ws::text(ph::CLOCK, 14.0, t.text_dim));
                        ws::divider(ui, &t, ws::APP_BAR_H - 8.0);
                        let buttons: [(&str, &str, bool, &str); 6] = [
                            (ph::QUESTION, "Help", false, "Help"),
                            (ph::GEAR_SIX, "Settings", false, "Settings"),
                            (ph::WRENCH, "Tools", false, "Tools"),
                            (ph::CHAT_TEXT, "Discussion", false, "Discussion"),
                            (ph::PLAY, "Playback", self.dock.timeline_open, "Playback"),
                            (ph::INFO, "Inspector", self.dock.inspector_open, "Inspector"),
                        ];
                        for (glyph, label, on, name) in buttons {
                            let label = if fit.labels { label } else { "" };
                            if ws::icon_button(ui, &t, glyph, label, on)
                                .named_toggle(name, on)
                                .clicked()
                            {
                                match name {
                                    "Inspector" => {
                                        self.dock.inspector_open = !self.dock.inspector_open;
                                    }
                                    "Playback" => {
                                        self.dock.timeline_open = !self.dock.timeline_open;
                                    }
                                    // The forecast *discussion* (AFD); model forecasts are a tab.
                                    "Discussion" => action = Some(A::OpenWindow(AppWindow::Afd)),
                                    "Tools" => {
                                        action = Some(A::OpenWindow(AppWindow::LayerManager));
                                    }
                                    "Settings" => {
                                        action = Some(A::OpenWindow(AppWindow::Settings));
                                    }
                                    _ => action = Some(A::OpenWindow(AppWindow::Help)),
                                }
                            }
                        }
                    });
                });
            });
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }

    /// The context toolbar: site, product, tilt, view mode and the overlays an analyst flips most,
    /// in one row that scrolls sideways rather than wrapping when the window is narrow.
    pub(super) fn dock_toolbar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::{OverlayToggle as T, PaletteAction as A};
        let t = self.ws_tokens();
        let toggles: Vec<(T, &str, bool)> = [
            (T::RangeRings, "Range rings"),
            (T::Alerts, "Warnings"),
            (T::Tracks, "Storm tracks"),
            (T::GlmLightning, "Lightning"),
        ]
        .into_iter()
        .map(|(tg, n)| (tg, n, *self.overlay_flag(tg)))
        .collect();
        let basemap = if self.settings.basemap.is_empty() {
            "Default".to_string()
        } else {
            self.settings.basemap.clone()
        };
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(view, _, _, _)| *view == self.active);
        let sweeping = sweeping_tilt(
            self.views[self.active].live_progress,
            streaming,
            self.settings.live_scan_indicator,
        );
        let (site, vcp, moment, srv, tilt, elevations, map_3d, follow_low) = {
            let v = &self.views[self.active];
            (
                v.site.clone(),
                v.volume
                    .as_ref()
                    .map(|x| vcp_number(&x.vcp))
                    .unwrap_or_default(),
                v.moment,
                v.srv,
                v.tilt,
                v.volume
                    .as_ref()
                    .map(|x| x.elevations.clone())
                    .unwrap_or_default(),
                v.map_3d.enabled,
                v.follow_lowest_cut,
            )
        };
        let table_key = moment.short_name();
        let table_now = self
            .settings
            .palettes
            .get(table_key)
            .map(|v| table_label(v))
            .unwrap_or_else(|| "Default".to_string());
        let mut smoothing = self.views[self.active].smooth;
        let mut action = None;
        let mut pick_tilt = None;
        let mut flip_follow = false;
        let mut want_3d = None;
        let mut open_sites = false;
        let mut table_pick: Option<Option<String>> = None;
        egui::Panel::top("dock_toolbar")
            .exact_size(ws::TOOLBAR_H)
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(egui::Stroke::new(1.0, t.line))
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                egui::ScrollArea::horizontal()
                    .id_salt("dock_toolbar_scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_height(ws::TOOLBAR_H);
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ws::caption(ui, &t, "Site");
                            let site_label = format!(
                                "{}  {}",
                                site.as_deref().unwrap_or("None"),
                                ph::CARET_DOWN
                            );
                            if ws::button(ui, &t, &site_label, 76.0)
                                .named("Choose a radar site")
                                .clicked()
                            {
                                open_sites = true;
                            }
                            if !vcp.is_empty() {
                                ws::caption(ui, &t, "VCP");
                                ui.label(ws::mono(&vcp, 12.0, t.text));
                            }
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            ws::caption(ui, &t, "Product");
                            egui::ComboBox::from_id_salt("dock_product")
                                .width(170.0)
                                .selected_text(crate::products::name(moment, srv))
                                .show_ui(ui, |ui| {
                                    for m in Moment::ALL {
                                        let on = m == moment;
                                        let name = crate::products::info(m).name;
                                        if ui.selectable_label(on, name).clicked() {
                                            action = Some(A::SetMoment(m, srv));
                                        }
                                    }
                                });
                            ws::caption(ui, &t, "Tilt");
                            let tilt_text = elevations.get(tilt).map_or_else(
                                || "\u{2014}".to_string(),
                                |a| format!("{a:.1}\u{b0}"),
                            );
                            egui::ComboBox::from_id_salt("dock_tilt")
                                .width(64.0)
                                .selected_text(tilt_text)
                                .show_ui(ui, |ui| {
                                    for (i, a) in elevations.iter().enumerate() {
                                        let mut label = format!("{a:.1}\u{b0}");
                                        if sweeping == Some(i) {
                                            label.push_str("  \u{25cf} live");
                                        }
                                        if ui.selectable_label(i == tilt, label).clicked() {
                                            pick_tilt = Some(i);
                                        }
                                    }
                                    ui.separator();
                                    if ui
                                        .selectable_label(false, "All tilts (four panes)")
                                        .clicked()
                                    {
                                        action = Some(A::AllTilts);
                                    }
                                });
                            let mut follow = follow_low;
                            if ws::check(ui, &t, &mut follow, "Follow lowest")
                                .on_hover_text(
                                    "While following live, jump to the lowest tilt the instant \
                                     it is rescanned (SAILS/MRLE)",
                                )
                                .changed()
                            {
                                flip_follow = true;
                            }
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            match ws::segmented(
                                ui,
                                &t,
                                &["2D", "3D", "Volume"],
                                usize::from(map_3d),
                            ) {
                                Some(0) => want_3d = Some(false),
                                Some(1) => want_3d = Some(true),
                                Some(_) => action = Some(A::OpenWindow(AppWindow::Volume3d)),
                                None => {}
                            }
                            ws::check(ui, &t, &mut smoothing, "Smoothing");
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            ws::caption(ui, &t, "Color table");
                            egui::ComboBox::from_id_salt("dock_table")
                                .width(120.0)
                                .selected_text(&table_now)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(table_now == "Default", "Default")
                                        .clicked()
                                    {
                                        table_pick = Some(None);
                                    }
                                    for name in crate::colormap::alt_names(moment) {
                                        if ui.selectable_label(table_now == name, name).clicked() {
                                            table_pick = Some(Some(format!(
                                                "{}{name}",
                                                crate::colormap::BUILTIN_PREFIX
                                            )));
                                        }
                                    }
                                });
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            for (tg, name, on) in &toggles {
                                let mut v = *on;
                                if ws::check(ui, &t, &mut v, name).changed() {
                                    action = Some(A::ToggleOverlay(*tg));
                                }
                            }
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            ws::caption(ui, &t, "Map");
                            if ws::button(ui, &t, &basemap, 72.0)
                                .named("Next map style")
                                .clicked()
                            {
                                action = Some(A::CycleBasemap);
                            }
                        });
                    });
            });
        self.views[self.active].smooth = smoothing;
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if flip_follow {
            let f = &mut self.views[self.active].follow_lowest_cut;
            *f = !*f;
        }
        if let Some(on) = want_3d {
            self.views[self.active].set_map_3d(on);
        }
        if open_sites {
            self.site_dialog = Some(Default::default());
        }
        if let Some(pick) = table_pick {
            // The palette watcher reloads the tables when the saved map changes.
            match pick {
                Some(v) => {
                    self.settings.palettes.insert(table_key.to_string(), v);
                }
                None => {
                    self.settings.palettes.remove(table_key);
                }
            }
        }
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }
}

/// What the app bar has room for at a window width: the panel buttons' words, the subtitle, and
/// the date beside the clock go, in that order, before anything would overlap the tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AppBarFit {
    pub labels: bool,
    pub subtitle: bool,
    pub date: bool,
}

pub(super) fn app_bar_fit(width: f32) -> AppBarFit {
    AppBarFit {
        labels: width >= 1780.0,
        subtitle: width >= 1400.0,
        date: width >= 1240.0,
    }
}

/// "VCP 212 (Precipitation, SZ-2)" → "212": the toolbar has room for the number alone, and the
/// Inspector says the rest.
pub(super) fn vcp_number(vcp: &str) -> String {
    vcp.split(" (")
        .next()
        .unwrap_or_default()
        .trim_start_matches("VCP")
        .trim()
        .to_string()
}

/// A saved colour-table choice as the toolbar names it: a built-in alternate by its own name, a
/// file by its file name.
pub(super) fn table_label(saved: &str) -> String {
    if let Some(name) = saved.strip_prefix(crate::colormap::BUILTIN_PREFIX) {
        return name.to_string();
    }
    std::path::Path::new(saved)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(saved)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrower_window_sheds_words_before_anything_overlaps() {
        let wide = app_bar_fit(1936.0);
        assert!(wide.labels && wide.subtitle && wide.date);
        let laptop = app_bar_fit(1536.0);
        assert!(!laptop.labels && laptop.subtitle && laptop.date);
        let tablet = app_bar_fit(1180.0);
        assert!(!tablet.labels && !tablet.subtitle && !tablet.date);
    }

    #[test]
    fn the_toolbar_shows_the_vcp_number_alone() {
        assert_eq!(vcp_number("VCP 212 (Precipitation, SZ-2)"), "212");
        assert_eq!(vcp_number("VCP 35"), "35");
        assert_eq!(vcp_number(""), "");
    }

    #[test]
    fn a_colour_table_is_named_by_its_builtin_name_or_file_name() {
        let builtin = format!("{}GR2 Classic", crate::colormap::BUILTIN_PREFIX);
        assert_eq!(table_label(&builtin), "GR2 Classic");
        assert_eq!(table_label("C:/tables/my_ref.pal"), "my_ref.pal");
        assert_eq!(table_label("Imported table"), "Imported table");
    }
}
