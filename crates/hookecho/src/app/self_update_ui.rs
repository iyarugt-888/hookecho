//! The self-update card (`crate::self_update`): checks the CI build once per launch, offers a
//! newer one in a small card over the map, downloads and checksums it on Install, then installs.

use super::*;
use crate::self_update::{self as su, State};
use std::sync::atomic::{AtomicBool, Ordering};

/// The launch check has run (or been skipped) this session.
static CHECKED: AtomicBool = AtomicBool::new(false);
/// Install as soon as the download is verified: set by the card's Install button.
static INSTALL_WHEN_READY: AtomicBool = AtomicBool::new(false);

impl HookEchoApp {
    /// Ask the rolling CI pre-release whether a newer build exists. Off the UI thread.
    pub(crate) fn check_for_build(&mut self, ctx: &egui::Context) {
        let Some(local) = su::local_build() else {
            su::set_state(State::DevBuild);
            return;
        };
        if su::asset_name().is_none() {
            su::set_state(State::Unsupported);
            return;
        }
        su::set_state(State::Checking);
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (ctx, local);
            su::set_state(State::Unsupported);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let http = self.http.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let ctx = ctx.clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.spawner.spawn(async move {
            let next = match su::fetch_info(&http).await {
                Ok(info) if su::is_newer(Some(local), &info) => State::Available(info),
                Ok(_) => State::UpToDate,
                Err(e) => State::Failed(format!("could not check for a new build: {e}")),
            };
            su::set_state(next);
            ctx.request_repaint();
        });
    }

    fn download_build(&mut self, info: su::BuildInfo, ctx: &egui::Context) {
        let (Some(name), Some(dir)) = (su::asset_name(), su::download_dir()) else {
            su::set_state(State::Failed("nowhere to download the update".into()));
            return;
        };
        su::set_state(State::Downloading {
            info: info.clone(),
            done: 0,
            total: None,
        });
        #[cfg(target_arch = "wasm32")]
        let _ = (ctx, name, dir, info);
        #[cfg(not(target_arch = "wasm32"))]
        let http = self.http.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let ctx = ctx.clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.spawner.spawn(async move {
            let next = match su::download(&http, &info, name, dir).await {
                Ok(path) => State::Ready { info, path },
                Err(e) => State::Failed(format!("download failed: {e}")),
            };
            su::set_state(next);
            ctx.request_repaint();
        });
    }

    /// Once per frame: the launch check, installing a verified download the user asked for, and
    /// the card while there is something to say.
    pub(crate) fn self_update_frame(&mut self, ctx: &egui::Context) {
        if cfg!(target_arch = "wasm32") {
            return; // the web build updates itself on reload
        }
        if self.settings.check_builds && !CHECKED.swap(true, Ordering::Relaxed) {
            self.check_for_build(ctx);
        }
        let state = su::state();
        if let State::Ready { path, .. } = &state {
            if INSTALL_WHEN_READY.swap(false, Ordering::Relaxed) {
                // On Windows this does not return: the app quits for the new build.
                if let Err(e) = su::install(path) {
                    su::set_state(State::Failed(e));
                }
            }
        }
        let show = match &state {
            State::Available(_) | State::Downloading { .. } | State::Ready { .. } => true,
            // A failure only after the user asked for something; a quiet launch check stays
            // quiet when offline.
            State::Failed(_) => INSTALLING.load(Ordering::Relaxed),
            _ => false,
        };
        if !show {
            return;
        }
        if matches!(state, State::Downloading { .. }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let t = self.ws_tokens();
        let mut action = None;
        egui::Area::new(egui::Id::new("self_update_card"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -56.0])
            .show(ctx, |ui| {
                crate::ui::workstation::card_frame(&t).show(ui, |ui| {
                    crate::ui::workstation::style_scope(ui, &t);
                    ui.set_width(280.0);
                    action = card(ui, &state);
                });
            });
        match action {
            Some(Card::Install(info)) => {
                INSTALLING.store(true, Ordering::Relaxed);
                INSTALL_WHEN_READY.store(true, Ordering::Relaxed);
                match su::state() {
                    State::Ready { .. } => {}
                    _ => self.download_build(info, ctx),
                }
            }
            Some(Card::Later(build)) => {
                INSTALLING.store(false, Ordering::Relaxed);
                su::set_state(State::Dismissed(build));
            }
            None => {}
        }
    }
}

/// The user asked to install this session: failures from here on are shown, not swallowed.
static INSTALLING: AtomicBool = AtomicBool::new(false);

enum Card {
    Install(su::BuildInfo),
    Later(u64),
}

fn card(ui: &mut egui::Ui, state: &State) -> Option<Card> {
    let mut out = None;
    match state {
        State::Available(info) | State::Ready { info, .. } => {
            ui.label(egui::RichText::new("A new HookEcho build is ready").strong());
            ui.weak(format!(
                "Build #{} ({}) · you have #{}",
                info.build,
                info.short_sha(),
                su::local_build().unwrap_or(0)
            ));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let label = if cfg!(target_os = "android") {
                    "Install"
                } else {
                    "Install and restart"
                };
                if ui.button(label).clicked() {
                    out = Some(Card::Install(info.clone()));
                }
                if ui.button("Later").clicked() {
                    out = Some(Card::Later(info.build));
                }
            });
        }
        State::Downloading { info, done, total } => {
            ui.label(egui::RichText::new(format!("Downloading build #{}", info.build)).strong());
            let frac = total.map_or(0.0, |t| *done as f32 / t.max(1) as f32);
            ui.add(egui::ProgressBar::new(frac).text(format!(
                "{:.1} MB{}",
                *done as f64 / 1e6,
                total.map_or(String::new(), |t| format!(" of {:.1}", t as f64 / 1e6))
            )));
        }
        State::Failed(msg) => {
            ui.label(egui::RichText::new("Update failed").strong());
            ui.weak(msg);
            if ui.button("Close").clicked() {
                out = Some(Card::Later(0));
            }
        }
        _ => {}
    }
    out
}
