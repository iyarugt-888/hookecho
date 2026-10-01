//! The frame's intake: the activity stamp, deep links, imported files, the local API, tray and
//! broker commands, run-in-background, the theme and the UI scale.
//! Moved out of `ui_frame` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn frame_intake(&mut self, ctx: &egui::Context) {
        // Stamp this frame for the background workers' foreground gate (see
        // `platform::activity`). A frame that follows a gap means the app just came back, so
        // force one refresh rather than making the user wait out the poll interval.
        self.frame_nr = self.frame_nr.wrapping_add(1);
        wxdata::stats::bump(wxdata::stats::Counter::FramesDrawn);
        self.gesture_live = ctx.input(map_gesture_live);
        #[cfg(not(target_arch = "wasm32"))]
        self.perf.tick(ctx);
        #[cfg(debug_assertions)]
        self.frame_time_overlay(ctx);
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if crate::platform::activity::mark_frame(focused) {
            self.overlay_last_fetch = None;
            for v in &mut self.views {
                v.last_poll = None;
            }
            // A resume is also how a notification tap arrives: the activity wrote the target
            // before handing us back the surface.
            self.drain_goto_file();
        }
        // A deep link that arrives while the app is already on screen never produces a resume, so
        // the drain above would never see it — on Android the activity is reused
        // (`launchMode="singleTask"`) and only `onNewIntent` fires; on desktop the second process
        // hands its link over and exits. ponytail: a one-second stat of a path that usually does
        // not exist, rather than a callback into the event loop.
        if self
            .goto_poll
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1))
        {
            self.goto_poll = Some(Instant::now());
            self.drain_goto_file();
            #[cfg(target_arch = "wasm32")]
            self.apply_goto_hash();
        }
        // The settings window has no HTTP client or runtime, so the voice-download button raises
        // a flag and the work happens here, on the same spawner everything else fetches on.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(id) = crate::speech::take_voice_request() {
            self.download_voice(id);
        }
        // A file the user picked, from any of the import buttons. Routed here rather than at the
        // button, because on Android the picker is an activity result that lands long after the
        // click — through the same file handover a notification tap uses.
        #[cfg(not(target_arch = "wasm32"))]
        self.tick_local_api(ctx);
        if let Some(import) = crate::dialog::take_result() {
            // A case restores panes, which needs the context the other imports do not.
            if import.kind == crate::dialog::ImportKind::Case {
                self.open_case(&import, ctx);
            } else {
                self.apply_import(import);
            }
        }

        // Android paste: re-focus the text field that lost focus to the Paste-button tap, before
        // any window draws, so the queued Paste event (see `raw_input_hook`) lands in it.
        if let Some(id) = self.paste_target.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }

        // Tray menu commands (Linux StatusNotifier): restore the window or quit for real.
        // Keep the tray menu telling the truth: alert count, mute state, starred sites. Sent only
        // when it changes — every send is a D-Bus round trip on the tray thread.
        {
            let want = crate::tray::TrayState {
                alerts: self.active_alert_features().len(),
                muted: self.settings.mute_alerts,
                starred: self.settings.presets.clone(),
            };
            if self.tray_state != want {
                self.tray_state = want.clone();
                crate::tray::set_state(want);
            }
        }
        {
            // Drained first: the handlers below need `&mut self`, and the receiver lives in it.
            let cmds: Vec<crate::tray::TrayCmd> = self.tray_rx.try_iter().collect();
            for cmd in cmds {
                match cmd {
                    crate::tray::TrayCmd::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    crate::tray::TrayCmd::Quit => {
                        self.really_quit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    crate::tray::TrayCmd::Mute => {
                        self.apply_action(BindableAction::ToggleMute, ctx);
                    }
                    crate::tray::TrayCmd::Site(id) => {
                        // Same path the site dialog uses: `sync_pane` does the rest next frame.
                        self.views[self.active].site = Some(id);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
        }

        // Strikes age out on their own clock: the deque only shrinks here, so a quiet topic
        // still empties the map rather than freezing the last minute of a storm on it.
        if !self.strikes.is_empty() {
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(STRIKE_WINDOW_SECS);
            while self.strikes.front().is_some_and(|&(_, _, t)| t < cutoff) {
                self.strikes.pop_front();
            }
        }

        // Commands off the broker, drained the same way and applied through the same paths the
        // tray uses. They land on the next repaint rather than instantly, which for "point at the
        // storm" is close enough and keeps every state change on one thread.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        for cmd in crate::mqtt::drain() {
            match cmd {
                crate::mqtt::Cmd::Mute(want) => {
                    if self.settings.mute_alerts != want {
                        self.apply_action(BindableAction::ToggleMute, ctx);
                    }
                }
                crate::mqtt::Cmd::Site(id) => {
                    if wxdata::sites::site_by_id(&id).is_some() {
                        self.views[self.active].site = Some(id);
                    } else {
                        log::warn!("mqtt: no such site {id}");
                    }
                }
                crate::mqtt::Cmd::Product(code) => {
                    if let Some(m) = Moment::from_code(&code) {
                        let srv = self.views[self.active].srv;
                        self.apply_palette(PaletteAction::SetMoment(m, srv), ctx);
                    }
                }
                crate::mqtt::Cmd::Strike { lon, lat, time } => {
                    self.strikes.push_back((lon, lat, time));
                    // A busy night over a whole continent is a lot of strikes, and the painter
                    // walks the whole deque. Cap it and let the oldest fall off early.
                    while self.strikes.len() > STRIKE_CAP {
                        self.strikes.pop_front();
                    }
                }
            }
        }

        // Run-in-background: when the user closes the window and close-to-tray is on (and it wasn't
        // a tray "Quit"), cancel the quit and hide instead — the app keeps polling alerts and
        // pushing ntfy. Restore via the tray icon (or the taskbar when no tray host is present).
        if self.settings.close_to_tray
            && !self.really_quit
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let cmd = if self.tray_present.load(std::sync::atomic::Ordering::Relaxed) {
                egui::ViewportCommand::Visible(false) // hide fully; the tray restores it
            } else {
                egui::ViewportCommand::Minimized(true) // no tray → keep a taskbar entry
            };
            ctx.send_viewport_cmd(cmd);
        }

        // "Modern dark pro" styling (palette/spacing/rounding/accent). Re-applied when the theme
        // or the system light/dark preference changes — it rebuilds and installs a whole
        // `egui::Style`, which is wasted work on every other frame.
        let system_dark = ctx.input(|i| i.raw.system_theme) != Some(egui::Theme::Light);
        let theme_key = (
            self.settings.theme,
            system_dark,
            self.settings.density,
            self.settings.accent,
        );
        if self.theme_applied != Some(theme_key) {
            crate::theme::apply(
                ctx,
                self.settings.theme,
                system_dark,
                self.settings.density,
                self.settings.accent,
            );
            self.theme_applied = Some(theme_key);
        }

        // UI scale: apply the setting when the slider moved, else absorb built-in keyboard zoom
        // (Ctrl+= / Ctrl+- / Ctrl+0) back into the setting so it persists.
        if (self.settings.ui_scale - self.ui_scale_applied).abs() > 1e-3 {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.ui_scale_applied = self.settings.ui_scale;
        } else {
            let z = ctx.zoom_factor();
            self.settings.ui_scale = z;
            self.ui_scale_applied = z;
        }
    }
}
