//! The end of the frame: settings persistence, the embed handover, the Android soft keyboard, the
//! mini loop, output and crash windows, and the idle repaint.
//! Moved out of `ui_frame` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn frame_end(&mut self, ctx: &egui::Context, root: &mut egui::Ui) {
        // Dirty-diff persistence: one write per actual change, from any mutation site. The
        // comparison walks the whole settings tree (palettes, placefiles, markers), so it runs at
        // most once a second rather than every frame; a change waits under a second to reach disk.
        let due = self
            .settings_checked
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if due {
            self.settings_checked = Some(Instant::now());
            // Fold the live overlay toggles into the settings so the diff below persists them
            // like any other change — no separate save path, no per-frame churn.
            let on: Vec<String> = OverlayToggle::ALL
                .into_iter()
                // Camera linking is a session decision about the panes on screen, not a layer.
                .filter(|t| !t.session_only() && *self.overlay_flag(*t))
                .map(|t| t.slug())
                .collect();
            self.settings.overlays_on = Some(on);
            // And the model contours: they were the one kind of layer a restart forgot.
            self.settings.contours_on = self
                .active_contours
                .iter()
                .filter_map(|k| k.token())
                .map(str::to_string)
                .collect();
            // Same trick for the window: fold the live size in, and the ordinary diff-and-save
            // below persists it.
            //
            // Measured from the root `Ui`, not `ViewportInfo::inner_rect`: on Wayland the
            // compositor never tells a client where its window is, so `inner_rect` is `None`
            // there and this silently saved nothing at all. The root ui covers the whole
            // viewport, in egui points — logical points divided by the ui-scale zoom — so
            // multiplying the zoom back gives the units `with_inner_size` wants.
            //
            // While maximized the size on screen is the screen's, not the one to restore to, so
            // the previous size is kept and only the flag moves.
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            {
                let (maximized, minimized) = root.ctx().input(|i| {
                    (
                        i.viewport().maximized.unwrap_or(false),
                        i.viewport().minimized.unwrap_or(false),
                    )
                });
                let size = root.max_rect().size() * root.ctx().zoom_factor();
                if !minimized && size.x > 1.0 && size.y > 1.0 {
                    let (width, height) = match self.settings.window {
                        Some(w) if maximized => (w.width, w.height),
                        _ => (size.x, size.y),
                    };
                    self.settings.window = Some(crate::settings::WindowGeom {
                        width,
                        height,
                        maximized,
                    });
                }
            }
        }
        if due && self.settings != self.saved {
            // A palette-map change reloads the color tables (bumps gen -> LUT re-bake).
            if self.settings.palettes != self.saved.palettes {
                self.palettes.reload(&self.settings.palette_paths());
            }
            self.settings.save();
            self.saved = self.settings.clone();
        }

        // Embedded in another page: hand the active pane's state to the parent frame so it can
        // persist it. Our own localStorage is partitioned (or wiped) inside a third-party iframe,
        // so the host is the only place this survives a reload. Same once-a-second tick as above,
        // and only when something actually moved.
        #[cfg(target_arch = "wasm32")]
        if due && self.embed {
            self.post_state_to_parent();
        }

        // Android text input: summon/dismiss the soft keyboard as egui focus moves in/out of
        // text fields, and float a Paste button (the system clipboard is unreachable from the
        // soft keyboard otherwise — egui gets the text as a Paste event next frame).
        if cfg!(target_os = "android") {
            // Only a text field wants the soft keyboard. Any focused widget used to count, so
            // tapping a checkbox raised the keyboard and reset the IME buffer under the field.
            let wants = ctx.text_edit_focused();
            if wants != self.ime_shown {
                crate::platform::show_soft_input(wants);
                self.ime_shown = wants;
            }
            if wants {
                egui::Area::new(egui::Id::new("android_paste_bar"))
                    .anchor(egui::Align2::RIGHT_TOP, [-8.0, 64.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            if ui.button("Paste").clicked() {
                                self.pending_paste = crate::platform::clipboard_text();
                                // Remember the field losing focus to this tap, to restore it next
                                // frame so the Paste event has somewhere to land.
                                self.paste_target = ui.ctx().memory(|m| m.focused());
                            }
                        });
                    });
            }
        }

        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        self.mini_loop_viewport(ctx);
        self.output_window(ctx);

        self.crash_report_window(ctx);

        // Unconditional, every frame, regardless of which (if any) drawer-hosted page ran above:
        // the self-pruning inside `Drawer::page`/`page_sized` only fires when *some* page calls
        // it, which is exactly the property the empty case doesn't have. Without this, closing
        // the last open page (Sounding, Settings, X-section, …) left the drawer's stack holding
        // its title forever, `Drawer::is_open()` reading true forever, and the floating
        // layers/alerts panel — which steps aside whenever a page is open — hidden until restart.
        self.drawer.end_frame(ctx);

        // Idle heartbeat so clocks (volume age, countdowns) tick without input. Data arrivals and
        // animations (pulse, banners) request faster repaints on their own. Slower on Android to
        // spare the battery — nothing on screen changes faster than this between frames.
        // An untouched embed is a still picture on someone else's dashboard: one frame a minute
        // keeps the clocks honest without spending the host's CPU. The first interaction wakes it
        // for good; data arrivals still request their own repaints either way.
        let busy = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down() || i.any_touches());
        if busy {
            self.last_input = Instant::now();
        }
        if self.embed && !self.embed_live && ctx.input(|i| i.pointer.any_down() || i.any_touches())
        {
            self.embed_live = true;
        }
        let idle = if self.embed && !self.embed_live {
            60_000
        } else if !crate::platform::activity::is_active() {
            2_000 // backgrounded: just enough to notice coming back
        } else if self.settings.battery_saver {
            // Four frames a second is still a live clock; it is not a live animation. Anything
            // that actually moves (a banner, a play head, an arriving volume) asks for its own
            // repaint and is unaffected.
            1_000
        } else if self.last_input.elapsed() > QUIET_AFTER {
            // Nobody has touched it for a while. This is a floor, not a schedule: egui takes the
            // *minimum* of every repaint request in a frame, so playback, the warning pulse, the
            // wind field and every arriving volume all still outbid it and animate at their own
            // rate. What it changes is the cost of a window nothing is happening in — ten wasted
            // full passes a second, each re-walking thirty poll clocks and re-projecting every
            // label, becomes two. The first event of any kind snaps it back the same frame.
            //
            // The visible price is that a clock can read up to 0.4 s stale in a still window.
            IDLE_QUIET_MS
        } else if cfg!(target_os = "android") {
            250
        } else {
            100
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.perf.idle_ms = idle;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(idle));
    }
}
