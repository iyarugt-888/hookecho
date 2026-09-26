//! The dock's two top bars: the app bar (brand, workspace tabs, panel buttons, clock and radar
//! health) and the context toolbar (the radar controls you reach for while looking at the map).

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// Current feed delay is the age of the newest known radar frame, not the ingest lag measured
/// when an earlier frame arrived. In archive mode that age is not a live-feed measurement.
fn radar_delay_label(
    state: &str,
    newest: Option<chrono::DateTime<chrono::Utc>>,
    following: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    if !following {
        return "Archive".to_string();
    }
    let Some(valid) = newest else {
        return state.to_string();
    };
    let seconds = (now - valid).num_seconds().max(0);
    let delay = if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    };
    format!("{state} · {delay} behind")
}

#[cfg(test)]
mod delay_tests {
    use super::radar_delay_label;

    #[test]
    fn live_delay_tracks_the_newest_frame_and_archive_does_not_claim_a_delay() {
        let now = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        let frame = now - chrono::Duration::seconds(81);
        assert_eq!(
            radar_delay_label("Fresh", Some(frame), true, now),
            "Fresh · 1m 21s behind"
        );
        assert_eq!(
            radar_delay_label("Stale", Some(frame), false, now),
            "Archive"
        );
        assert_eq!(radar_delay_label("Waiting", None, true, now), "Waiting");
        assert_eq!(
            radar_delay_label("Fresh", Some(now + chrono::Duration::seconds(2)), true, now),
            "Fresh · 0m 00s behind"
        );
    }
}

impl HookEchoApp {
    pub(super) fn dock_app_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let t = self.ws_tokens();
        // The feed delay includes seconds, so update it even on an idle map.
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let tz = self.active_tz();
        let now = chrono::Utc::now();
        let width = ctx.content_rect().width();
        let fit = app_bar_fit(width);
        let clock = if fit.date {
            crate::timefmt::fmt_date_clock(now, tz)
        } else {
            crate::timefmt::fmt_clock(now, tz, false)
        };
        // How wide the right-hand cluster came out last frame: the left side fits itself into
        // what that leaves rather than trusting a width table to predict both.
        let right_id = egui::Id::new("dock_app_bar_right_w");
        let right_w = ctx.data(|d| d.get_temp::<f32>(right_id)).unwrap_or(560.0);
        let tabs_w: f32 = DockTab::ALL
            .iter()
            .map(|tab| {
                ctx.fonts_mut(|f| {
                    f.layout_no_wrap(
                        tab.label().to_string(),
                        egui::FontId::proportional(13.5),
                        t.text,
                    )
                    .size()
                    .x
                }) + 24.0
            })
            .sum();
        let head = left_fit(width - 24.0 - right_w, tabs_w, fit.subtitle);
        let health = self.radar_health();
        let (state_word, state_color) = crate::ui::layers_panel::health_look(health.state());
        let following = self.views[self.active].timeline.following;
        let delay_text = radar_delay_label(state_word, health.latest_valid_time, following, now);
        let delay_tip = if following {
            match self.views[self.active].last_live_arrival {
                Some((arrived, valid)) => format!(
                    "Newest feed frame behind the current time. Last received frame was {} behind at arrival.",
                    humanize((arrived - valid).num_seconds().max(0))
                ),
                None => "Newest feed frame behind the current time. No live arrival measured yet.".to_string(),
            }
        } else {
            "Viewing the archive. Return to Live to see current feed delay.".to_string()
        };
        let keepout = if crate::os_decorated() {
            0.0
        } else {
            crate::ui::wsv3::WINDOW_BTN_KEEPOUT
        };
        let (alert_count, _) = self.alert_badge();
        let workspaces: Vec<String> = self
            .settings
            .workspaces
            .iter()
            .map(|w| w.name.clone())
            .collect();
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
                // The bar is the window's title bar: its empty space drags the window. Allocated
                // before the controls, so each of them keeps its own clicks.
                let caption = ui.interact(
                    ui.max_rect(),
                    ui.id().with("dock_caption"),
                    egui::Sense::click_and_drag(),
                );
                crate::app::chrome::window_frame::caption_drag(ctx, &caption);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(ws::text(ph::BROADCAST, 18.0, t.accent))
                        .on_hover_text("HookEcho");
                    ui.add_space(4.0);
                    if head.wordmark {
                        ui.label(ws::text("HookEcho", 14.5, egui::Color32::WHITE).strong());
                        ui.add_space(6.0);
                    }
                    if head.subtitle {
                        ui.label(ws::text("Analyst Workstation", 12.0, t.text_faint));
                    }
                    ui.add_space(if head.wordmark { 18.0 } else { 10.0 });
                    if !head.tabs {
                        // Too narrow for six tabs: one menu names the current one and lists all.
                        let current = self.dock.tab;
                        let menu = ws::icon_button(
                            ui,
                            &t,
                            ph::CARET_DOWN,
                            current.label(),
                            self.dock.shown(DockWin::Layers),
                        )
                        .named(&format!("Workspace tab: {}", current.label()));
                        egui::Popup::menu(&menu).show(|ui| {
                            for tab in DockTab::ALL {
                                if ui.selectable_label(tab == current, tab.label()).clicked() {
                                    self.dock.tab = tab;
                                    if !self.dock.shown(DockWin::Layers) {
                                        self.dock.toggle(DockWin::Layers);
                                    }
                                }
                            }
                        });
                    }
                    for tab in DockTab::ALL.into_iter().filter(|_| head.tabs) {
                        let on = self.dock.shown(DockWin::Layers) && self.dock.tab == tab;
                        if ws::tab(ui, &t, tab.label(), on, ws::APP_BAR_H)
                            .named_toggle(tab.label(), on)
                            .clicked()
                        {
                            // The open tab's own button folds the panel away; any other opens it
                            // on that tab.
                            if !on {
                                self.dock.tab = tab;
                            }
                            if on || !self.dock.shown(DockWin::Layers) {
                                self.dock.toggle(DockWin::Layers);
                            }
                        }
                    }
                    let right =
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let start = ui.cursor().right();
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.add_space(keepout);
                            if ui
                                .add(
                                    egui::Label::new(ws::mono(&delay_text, 12.0, t.text_dim))
                                        .sense(egui::Sense::click()),
                                )
                                .named("Show the health of every active source")
                                .on_hover_text(format!(
                                    "{}\nClick for every active source's health",
                                    health.error.as_deref().unwrap_or(&delay_tip)
                                ))
                                .clicked()
                            {
                                // The dock-width list; its footer opens the full table.
                                self.dock.toggle(DockWin::Sources);
                            }
                            ws::status_dot(ui, if following { state_color } else { t.warn }, 4.0);
                            ui.add_space(8.0);
                            if fit.clock {
                                ui.label(ws::mono(clock, 12.0, t.text));
                                ui.label(ws::text(ph::CLOCK, 14.0, t.text_dim));
                            }
                            ws::divider(ui, &t, ws::APP_BAR_H - 8.0);
                            use super::menus::{window_rows, Menu, MenuPick};
                            let mut menu_pick: Option<MenuPick> = None;
                            let label = |l: &'static str| if fit.labels { l } else { "" };
                            // Right to left: Help, Settings, Share, Tools, Discussion, Alerts,
                            // Playback, Inspector.
                            let help = ws::icon_button(ui, &t, ph::QUESTION, label("Help"), false)
                                .named("Help");
                            egui::Popup::menu(&help).show(|ui| {
                                ws::style_scope(ui, &t);
                                ui.set_min_width(220.0);
                                if let Some(p) = window_rows(ui, &t, Menu::Help) {
                                    menu_pick = Some(p);
                                }
                                if ui.button("Keyboard shortcuts (?)").clicked() {
                                    menu_pick = Some(MenuPick::Shortcuts);
                                }
                            });
                            let settings =
                                ws::icon_button(ui, &t, ph::GEAR_SIX, label("Settings"), false)
                                    .named("Settings");
                            egui::Popup::menu(&settings).show(|ui| {
                                ws::style_scope(ui, &t);
                                ui.set_min_width(220.0);
                                if let Some(p) = window_rows(ui, &t, Menu::Settings) {
                                    menu_pick = Some(p);
                                }
                                if ui.button("Map settings").clicked() {
                                    menu_pick = Some(MenuPick::Prefs(PrefsPage::Map, None));
                                }
                                if ui.button("Preferences").clicked() {
                                    menu_pick = Some(MenuPick::Prefs(PrefsPage::App, None));
                                }
                                ui.separator();
                                if ui.button("Hide the top bars (T)").clicked() {
                                    menu_pick = Some(MenuPick::Palette(A::ToggleRibbon));
                                }
                            });
                            let share = ws::icon_button(ui, &t, ph::EXPORT, label("Share"), false)
                                .named("Share, export and workspaces");
                            egui::Popup::menu(&share).show(|ui| {
                                ws::style_scope(ui, &t);
                                ui.set_min_width(240.0);
                                for (text, act) in [
                                    ("Copy a link to this view", A::CopyViewLink),
                                    ("Open in Windy", A::OpenInWindy),
                                    ("Export the map as GeoJSON", A::ExportGis),
                                    ("Import GeoJSON or Shapefile\u{2026}", A::ImportGis),
                                ] {
                                    if ui.button(text).clicked() {
                                        menu_pick = Some(MenuPick::Palette(act));
                                    }
                                }
                                if ui.button("Images, video and more\u{2026}").clicked() {
                                    menu_pick =
                                        Some(MenuPick::Prefs(PrefsPage::App, Some("Share")));
                                }
                                ui.separator();
                                ui.label(ws::text("WORKSPACES", 10.5, t.text_faint));
                                if ui.button("Save this layout as a workspace").clicked() {
                                    menu_pick = Some(MenuPick::Palette(A::SaveWorkspace));
                                }
                                for (i, name) in workspaces.iter().enumerate() {
                                    if ui.button(format!("Open \u{201c}{name}\u{201d}")).clicked() {
                                        menu_pick = Some(MenuPick::Palette(A::ApplyWorkspace(i)));
                                    }
                                }
                            });
                            let tools = ws::icon_button(ui, &t, ph::WRENCH, label("Tools"), false)
                                .named("Tools and windows");
                            egui::Popup::menu(&tools).show(|ui| {
                                ws::style_scope(ui, &t);
                                ui.set_min_width(240.0);
                                if let Some(p) = window_rows(ui, &t, Menu::Tools) {
                                    menu_pick = Some(p);
                                }
                            });
                            if ws::icon_button(ui, &t, ph::CHAT_TEXT, label("Discussion"), false)
                                .named("Forecast discussion (AFD)")
                                .clicked()
                            {
                                action = Some(A::OpenWindow(AppWindow::Afd));
                            }
                            let alerts_on = self.dock.shown(DockWin::Alerts);
                            let alerts_label = match (fit.labels, alert_count) {
                                (true, 0) => "Alerts".to_string(),
                                (true, n) => format!("Alerts {n}"),
                                (false, 0) => String::new(),
                                (false, n) => n.to_string(),
                            };
                            if ws::icon_button(ui, &t, ph::BELL, &alerts_label, alerts_on)
                                .named_toggle(&format!("Alerts in view: {alert_count}"), alerts_on)
                                .clicked()
                            {
                                self.dock.toggle(DockWin::Alerts);
                            }
                            let playing = self.dock.timeline_open;
                            if ws::icon_button(ui, &t, ph::PLAY, label("Playback"), playing)
                                .named_toggle("Playback", playing)
                                .clicked()
                            {
                                self.dock.timeline_open = !playing;
                            }
                            let inspecting = self.dock.shown(DockWin::Inspector);
                            if ws::icon_button(ui, &t, ph::INFO, label("Inspector"), inspecting)
                                .named_toggle("Inspector", inspecting)
                                .clicked()
                            {
                                self.dock.toggle(DockWin::Inspector);
                            }
                            match menu_pick {
                                Some(MenuPick::Palette(a)) => action = Some(a),
                                Some(MenuPick::Prefs(page, section)) => {
                                    self.dock.prefs.open = true;
                                    self.dock.prefs.collapsed = false;
                                    self.dock.prefs_page = page;
                                    ui.ctx().data_mut(|d| {
                                        let id = egui::Id::new("preferences_section");
                                        match section {
                                            Some(s) => {
                                                d.insert_temp(id, s);
                                            }
                                            None => d.remove::<&'static str>(id),
                                        }
                                    });
                                }
                                Some(MenuPick::Shortcuts) => self.show_cheatsheet = true,
                                None => {}
                            }
                            start - ui.cursor().right()
                        });
                    ctx.data_mut(|d| d.insert_temp(right_id, right.inner));
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
        let basemap = self.views[self.active].basemap.label();
        let basemap_open = self.basemap_open;
        let panes = self.views.len();
        let pane_layout = self.pane_layout;
        let (vcp_full, tilt_cuts) = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| (v.vcp.clone(), wxdata::level2::tilt_cuts(&v.scan)))
            .unwrap_or_default();
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(view, _, _, _)| *view == self.active);
        let live_progress = self.views[self.active].live_progress;
        let (site, vcp, moment, srv, tilt, elevations, map_3d, follow_now) = {
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
                Follow::of(v.follow_lowest_cut, v.follow_live_sweep),
            )
        };
        let sweeping = sweeping_tilt(
            live_progress,
            &elevations,
            streaming,
            self.settings.live_scan_indicator,
        );
        let table_key = moment.short_name();
        let table_now = self
            .settings
            .palettes
            .get(table_key)
            .map(|v| table_label(v))
            .unwrap_or_else(|| "Default".to_string());
        let mut smoothing = self.views[self.active].smooth;
        let mut legend = self.views[self.active].show_legend;
        let mut toggle_basemap = false;
        let mut action = None;
        let mut pick_tilt = None;
        let mut pick_follow: Option<Follow> = None;
        let mut want_3d = None;
        let mut open_sites = false;
        let mut table_pick: Option<Option<String>> = None;
        // The groups' widths as last drawn, to decide how many fold into the Display menu.
        let widths_id = egui::Id::new("dock_toolbar_widths");
        let mut widths = ctx
            .data(|d| d.get_temp::<ToolbarWidths>(widths_id))
            .unwrap_or_default();
        let folded = toolbar_folds(ctx.content_rect().width() - 20.0, &widths);
        let fold = |g: ToolGroup| (g as usize) < folded;
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
                            let x0 = ui.cursor().left();
                            // Each group's width, measured from where it starts to where the next
                            // one does.
                            let mut mark = x0;
                            let mut take = |ui: &egui::Ui| {
                                let x = ui.cursor().left();
                                let w = x - mark;
                                mark = x;
                                w
                            };
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
                                // The scan strategy in full, and which tilts it rescans, on click.
                                let r = ui
                                    .add(
                                        egui::Button::new(ws::mono(&vcp, 12.0, t.text))
                                            .frame(false),
                                    )
                                    .named("Scan strategy details");
                                egui::Popup::menu(&r).show(|ui| {
                                    ws::style_scope(ui, &t);
                                    crate::app::chrome::ribbon::scan_strategy_popup(
                                        ui, &vcp_full, &tilt_cuts,
                                    );
                                });
                            }
                            ws::divider(ui, &t, ws::TOOLBAR_H);
                            ws::caption(ui, &t, "Product");
                            egui::ComboBox::from_id_salt("dock_product")
                                .width(170.0)
                                .selected_text(crate::products::name(moment, srv))
                                .show_ui(ui, |ui| {
                                    for m in Moment::ALL {
                                        let on = m == moment && !(srv && m == Moment::Velocity);
                                        let name = crate::products::info(m).name;
                                        if ui.selectable_label(on, name).clicked() {
                                            action = Some(A::SetMoment(m, false));
                                        }
                                        // Storm-relative velocity sits right under velocity.
                                        if m == Moment::Velocity {
                                            let label = crate::products::name(m, true);
                                            if ui
                                                .selectable_label(moment == m && srv, label)
                                                .clicked()
                                            {
                                                action = Some(A::SetMoment(m, true));
                                            }
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
                            // What the radar is sweeping right now, and how far through the sweep
                            // it is — the ribbon's strip on the live tilt pill, as one chip.
                            if let (Some(i), Some(p)) = (sweeping, live_progress) {
                                let label = format!(
                                    "\u{25cf} {:.1}\u{b0}  {}/{}",
                                    p.elevation_angle_deg, p.chunk_index, p.chunks_in_sweep
                                );
                                let chip = ui
                                    .add(
                                        egui::Button::new(ws::mono(&label, 11.5, t.live))
                                            .fill(t.field)
                                            .stroke(egui::Stroke::new(1.0, t.line))
                                            .min_size(egui::vec2(0.0, ws::CONTROL_H)),
                                    )
                                    .named(&format!(
                                        "Live sweep at {:.1} degrees; show that tilt",
                                        p.elevation_angle_deg
                                    ))
                                    .on_hover_text(format!(
                                        "The radar is scanning {:.1}\u{b0} now: sweep {} of {}, \
                                         chunk {} of {}. Click to show that tilt.",
                                        p.elevation_angle_deg,
                                        p.elevation_number,
                                        p.total_elevations,
                                        p.chunk_index,
                                        p.chunks_in_sweep
                                    ));
                                crate::app::chrome::ribbon::live_sweep_strip(
                                    ui, chip.rect, p, t.live,
                                );
                                if chip.clicked() {
                                    pick_tilt = Some(i);
                                }
                            }
                            ws::caption(ui, &t, "Follow");
                            egui::ComboBox::from_id_salt("dock_follow")
                                .width(62.0)
                                .selected_text(follow_now.label())
                                .show_ui(ui, |ui| {
                                    for f in Follow::ALL {
                                        if ui
                                            .selectable_label(f == follow_now, f.label())
                                            .on_hover_text(f.hint())
                                            .clicked()
                                        {
                                            pick_follow = Some(f);
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(follow_now.hint());
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
                            widths.core = take(ui);
                            if !fold(ToolGroup::View) {
                                ws::check(ui, &t, &mut smoothing, "Smoothing");
                                ws::check(ui, &t, &mut legend, "Legend")
                                    .on_hover_text("The product's colour scale beside the map");
                                ws::divider(ui, &t, ws::TOOLBAR_H);
                                widths.groups[ToolGroup::View as usize] = take(ui);
                            }
                            if !fold(ToolGroup::Table) {
                                ws::caption(ui, &t, "Color table");
                                egui::ComboBox::from_id_salt("dock_table")
                                    .width(120.0)
                                    .selected_text(&table_now)
                                    .show_ui(ui, |ui| {
                                        table_items(ui, moment, &table_now, &mut table_pick);
                                    });
                                ws::divider(ui, &t, ws::TOOLBAR_H);
                                widths.groups[ToolGroup::Table as usize] = take(ui);
                            }
                            if !fold(ToolGroup::Overlays) {
                                for (tg, name, on) in &toggles {
                                    let mut v = *on;
                                    if ws::check(ui, &t, &mut v, name).changed() {
                                        action = Some(A::ToggleOverlay(*tg));
                                    }
                                }
                                ws::divider(ui, &t, ws::TOOLBAR_H);
                                widths.groups[ToolGroup::Overlays as usize] = take(ui);
                            }
                            if !fold(ToolGroup::Map) {
                                ws::caption(ui, &t, "Map");
                                let map_label = format!("{basemap}  {}", ph::CARET_DOWN);
                                if ws::button(ui, &t, &map_label, 72.0)
                                    .named("Choose the map style (Z cycles them)")
                                    .clicked()
                                {
                                    toggle_basemap = true;
                                }
                                ws::caption(ui, &t, "Panes");
                                egui::ComboBox::from_id_salt("dock_panes")
                                    .width(88.0)
                                    .selected_text(format!(
                                        "{panes} \u{b7} {}",
                                        pane_layout.label()
                                    ))
                                    .show_ui(ui, |ui| {
                                        pane_items(ui, panes, pane_layout, &mut action);
                                    });
                                widths.groups[ToolGroup::Map as usize] = take(ui);
                            }
                            if folded > 0 {
                                // Whatever did not fit, in the same order, behind one button.
                                let on = toggles.iter().filter(|(_, _, on)| *on).count();
                                let label = if fold(ToolGroup::Overlays) && on > 0 {
                                    format!("Display ({on})")
                                } else {
                                    "Display".to_string()
                                };
                                let menu =
                                    ws::icon_button(ui, &t, ph::SLIDERS_HORIZONTAL, &label, false)
                                        .named("Display settings that do not fit the toolbar");
                                egui::Popup::menu(&menu)
                                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                    .show(|ui| {
                                        ws::style_scope(ui, &t);
                                        if fold(ToolGroup::View) {
                                            ws::check(ui, &t, &mut smoothing, "Smoothing");
                                            ws::check(ui, &t, &mut legend, "Legend");
                                            ui.separator();
                                        }
                                        if fold(ToolGroup::Table) {
                                            ui.menu_button(
                                                format!("Color table: {table_now}"),
                                                |ui| {
                                                    table_items(
                                                        ui,
                                                        moment,
                                                        &table_now,
                                                        &mut table_pick,
                                                    );
                                                },
                                            );
                                            ui.separator();
                                        }
                                        if fold(ToolGroup::Overlays) {
                                            for (tg, name, on) in &toggles {
                                                let mut v = *on;
                                                if ws::check(ui, &t, &mut v, name).changed() {
                                                    action = Some(A::ToggleOverlay(*tg));
                                                }
                                            }
                                            ui.separator();
                                        }
                                        if fold(ToolGroup::Map) {
                                            if ui
                                                .button(format!("Map style: {basemap}\u{2026}"))
                                                .clicked()
                                            {
                                                toggle_basemap = true;
                                            }
                                            ui.menu_button(
                                                format!(
                                                    "Panes: {panes} \u{b7} {}",
                                                    pane_layout.label()
                                                ),
                                                |ui| {
                                                    pane_items(ui, panes, pane_layout, &mut action);
                                                },
                                            );
                                        }
                                    });
                            }
                        });
                    });
            });
        ctx.data_mut(|d| d.insert_temp(widths_id, widths));
        self.views[self.active].smooth = smoothing;
        self.views[self.active].show_legend = legend;
        if toggle_basemap {
            self.basemap_open = !basemap_open;
        }
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(f) = pick_follow {
            let v = &mut self.views[self.active];
            v.follow_lowest_cut = f == Follow::Lowest;
            v.follow_live_sweep = f == Follow::Sweep;
            v.followed_sweep = None;
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

/// What the displayed tilt follows while live: nothing (the tilt you pick), the lowest tilt each
/// time it is rescanned, or every sweep as the radar starts it. One choice, so the two automatic
/// modes can never both be on and fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Follow {
    Off,
    Lowest,
    Sweep,
}

impl Follow {
    const ALL: [Follow; 3] = [Follow::Off, Follow::Lowest, Follow::Sweep];

    fn of(lowest: bool, sweep: bool) -> Follow {
        if sweep {
            Follow::Sweep
        } else if lowest {
            Follow::Lowest
        } else {
            Follow::Off
        }
    }

    fn label(self) -> &'static str {
        match self {
            Follow::Off => "Off",
            Follow::Lowest => "Lowest",
            Follow::Sweep => "Sweep",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Follow::Off => "The tilt stays where you put it",
            Follow::Lowest => {
                "While live, jump to the lowest tilt the instant it is rescanned (SAILS/MRLE)"
            }
            Follow::Sweep => {
                "While live, change tilt as each new sweep starts, showing the elevation the \
                 radar is scanning; a tilt you pick holds until the next sweep"
            }
        }
    }
}

/// The context toolbar's foldable groups, in the order they fold into its Display menu when the
/// window is too narrow for them: the overlay switches first (the rail and Layers panel reach
/// them too), then the map style and panes, the colour table, and last smoothing and the legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolGroup {
    Overlays = 0,
    Map = 1,
    Table = 2,
    View = 3,
}

/// The toolbar's widths as last drawn: the fixed part (site to the 2D/3D/Volume switch) and each
/// foldable group, indexed by [`ToolGroup`]. The defaults are the desktop's, for the first frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ToolbarWidths {
    core: f32,
    groups: [f32; 4],
}

impl Default for ToolbarWidths {
    fn default() -> Self {
        ToolbarWidths {
            core: 780.0,
            groups: [390.0, 270.0, 230.0, 190.0],
        }
    }
}

/// How many groups fold (they go in [`ToolGroup`] order) for everything left to fit in `room`.
/// When even all four folded do not fit, the row still scrolls sideways.
fn toolbar_folds(room: f32, w: &ToolbarWidths) -> usize {
    const MENU: f32 = 110.0;
    (0..=4)
        .find(|&k| {
            let shown: f32 = w.groups[k..].iter().sum();
            w.core + shown + if k > 0 { MENU } else { 0.0 } <= room
        })
        .unwrap_or(4)
}

fn table_items(
    ui: &mut egui::Ui,
    moment: Moment,
    table_now: &str,
    pick: &mut Option<Option<String>>,
) {
    if ui
        .selectable_label(table_now == "Default", "Default")
        .clicked()
    {
        *pick = Some(None);
    }
    for name in crate::colormap::alt_names(moment) {
        if ui.selectable_label(table_now == name, name).clicked() {
            *pick = Some(Some(format!("{}{name}", crate::colormap::BUILTIN_PREFIX)));
        }
    }
}

fn pane_items(
    ui: &mut egui::Ui,
    panes: usize,
    pane_layout: crate::workspace::PaneLayout,
    action: &mut Option<crate::app::PaletteAction>,
) {
    use crate::app::PaletteAction as A;
    for n in [1usize, 2, 3, 4, 6, 9]
        .into_iter()
        .filter(|n| *n <= crate::view::MAX_PANES)
    {
        let label = if n == 1 {
            "1 pane".to_string()
        } else {
            format!("{n} panes")
        };
        if ui.selectable_label(panes == n, label).clicked() {
            *action = Some(A::SetPanes(n));
        }
    }
    ui.separator();
    for layout in crate::workspace::PaneLayout::ALL {
        if ui
            .selectable_label(pane_layout == layout, layout.label())
            .on_hover_text(layout.description())
            .clicked()
        {
            *action = Some(A::SetPaneLayout(layout));
        }
    }
}

/// What the app bar's right-hand cluster has room for at a window width: the buttons' words, the
/// date beside the clock, and then the wall clock itself (the timeline still shows the frame's
/// time) go, in that order. The left side then fits itself into what is left ([`left_fit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AppBarFit {
    pub labels: bool,
    pub subtitle: bool,
    pub date: bool,
    pub clock: bool,
}

pub(super) fn app_bar_fit(width: f32) -> AppBarFit {
    AppBarFit {
        labels: width >= 1900.0,
        subtitle: width >= 1400.0,
        date: width >= 1240.0,
        clock: width >= 1120.0,
    }
}

/// What the app bar's left side shows in `room` (the width the right-hand cluster left):
/// the six tabs come first, then the wordmark, then the subtitle; with no room for the tabs at
/// all they fold into one menu, which leaves room for the wordmark again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LeftFit {
    pub tabs: bool,
    pub wordmark: bool,
    pub subtitle: bool,
}

pub(super) fn left_fit(room: f32, tabs_w: f32, subtitle_wanted: bool) -> LeftFit {
    const GLYPH: f32 = 34.0;
    const WORDMARK: f32 = 80.0;
    const SUBTITLE: f32 = 130.0;
    const MENU: f32 = 120.0;
    let tabs = GLYPH + tabs_w <= room;
    let body = if tabs { tabs_w } else { MENU };
    let wordmark = GLYPH + WORDMARK + body <= room;
    LeftFit {
        tabs,
        wordmark,
        subtitle: subtitle_wanted && wordmark && GLYPH + WORDMARK + SUBTITLE + body <= room,
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
        assert!(!tablet.labels && !tablet.subtitle && !tablet.date && tablet.clock);
        assert!(!app_bar_fit(1024.0).clock);
    }

    #[test]
    fn the_left_side_gives_up_words_before_tabs_and_tabs_before_overlapping() {
        let tabs = 430.0;
        let all = left_fit(800.0, tabs, true);
        assert!(all.tabs && all.wordmark && all.subtitle);
        let no_sub = left_fit(560.0, tabs, true);
        assert!(no_sub.tabs && no_sub.wordmark && !no_sub.subtitle);
        let bare = left_fit(470.0, tabs, true);
        assert!(bare.tabs && !bare.wordmark);
        // Too narrow for the tabs even bare: one menu, which frees room for the name again.
        let menu = left_fit(300.0, tabs, true);
        assert!(!menu.tabs && menu.wordmark && !menu.subtitle);
        assert!(
            !left_fit(800.0, tabs, false).subtitle,
            "the right side vetoes it"
        );
    }

    #[test]
    fn follow_is_one_choice_of_three() {
        assert_eq!(Follow::of(false, false), Follow::Off);
        assert_eq!(Follow::of(true, false), Follow::Lowest);
        // Both set (an older state) reads as the broader mode.
        assert_eq!(Follow::of(true, true), Follow::Sweep);
    }

    #[test]
    fn the_toolbar_folds_groups_in_order_until_the_rest_fits() {
        let w = ToolbarWidths {
            core: 700.0,
            groups: [400.0, 250.0, 200.0, 150.0],
        };
        assert_eq!(toolbar_folds(1700.0, &w), 0);
        // One short: the overlays go first, and the menu that takes them costs room too.
        assert_eq!(toolbar_folds(1690.0, &w), 1);
        assert_eq!(toolbar_folds(1210.0, &w), 2);
        assert_eq!(toolbar_folds(810.0, &w), 4);
        // Narrower than even the fixed part: everything folds and the row scrolls.
        assert_eq!(toolbar_folds(500.0, &w), 4);
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
