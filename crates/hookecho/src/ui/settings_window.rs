//! Settings window: General, Appearance, Palettes, Units, Basemaps, Alerts, Hotkeys, Sync, Storage.

use crate::app::PaletteEntry;
use crate::colormap::Palettes;
use crate::hotkeys::{self, BindableAction, Binding};
use crate::settings::{Settings, TimeDisplay, VelocityUnit};
use crate::ui::a11y::Named as _;
use wxdata::level2::Moment;

#[derive(Debug, Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    General,
    /// theme_plan.md §3: split out of General — theme_plan.md §6.1's "Theme"/"Color scheme"
    /// split, the timeline style, and the floating-search toggle live here now.
    Appearance,
    Palettes,
    Units,
    Basemaps,
    Alerts,
    Hotkeys,
    Sync,
    Storage,
}

#[cfg(test)]
mod dock_tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes Settings captures for visual review"]
    fn gpu_settings_dock_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for UI review");
        let destination =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-review");
        std::fs::create_dir_all(&destination).unwrap();
        for width in [280, 640] {
            for (tab, name) in [
                (Tab::General, "general"),
                (Tab::Appearance, "appearance"),
                (Tab::Palettes, "palettes"),
                (Tab::Units, "units"),
                (Tab::Basemaps, "basemaps"),
                (Tab::Alerts, "alerts"),
                (Tab::Hotkeys, "hotkeys"),
                (Tab::Sync, "sync"),
                (Tab::Storage, "storage"),
            ] {
                let mut window = SettingsWindow {
                    open: true,
                    tab,
                    scanned: true,
                    storage_rows: Some(vec![]),
                    ..Default::default()
                };
                let mut settings = Settings {
                    layout: crate::settings::Layout::Dock,
                    theme: crate::settings::Theme::DearImGui,
                    ..Default::default()
                };
                let palettes = Palettes::default();
                let tokens =
                    crate::ui::workstation::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
                gpu.save(
                    &destination.join(format!("settings-{name}-{width}.png")),
                    width,
                    720,
                    |ui| {
                        egui::Frame::NONE
                            .fill(tokens.panel)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                crate::ui::workstation::style_scope(ui, &tokens);
                                crate::ui::workstation::window_header(
                                    ui,
                                    &tokens,
                                    egui_phosphor::regular::GEAR,
                                    "Settings",
                                    Some(crate::workspace::Place::Float),
                                    Some(false),
                                );
                                window.show_body(
                                    ui,
                                    &mut settings,
                                    &palettes,
                                    SyncView {
                                        signed_in: false,
                                        status: "Offline",
                                        login_url: None,
                                        last_sync: 0,
                                    },
                                    &[],
                                    true,
                                );
                            });
                    },
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn closing_settings_releases_key_capture_and_rebinding() {
        let mut window = SettingsWindow {
            open: true,
            rebinding: Some(BindableAction::CommandSearch),
            capturing: true,
            ..Default::default()
        };
        window.prepare();
        window.open = false;
        window.prepare();
        assert!(!window.capturing && window.rebinding.is_none());
    }

    #[test]
    fn narrow_and_wide_dock_hosts_render_shared_settings_sections() {
        for width in [280.0, 620.0] {
            for tab in [Tab::Appearance, Tab::Units, Tab::Hotkeys, Tab::Sync] {
                let ctx = egui::Context::default();
                let mut window = SettingsWindow {
                    open: true,
                    tab,
                    ..Default::default()
                };
                let mut settings = Settings::default();
                let palettes = Palettes::default();
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 420.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        window.show_body(
                            ui,
                            &mut settings,
                            &palettes,
                            SyncView {
                                signed_in: false,
                                status: "Offline",
                                login_url: None,
                                last_sync: 0,
                            },
                            &[],
                            true,
                        );
                        assert!(
                            ui.min_rect().width() <= width,
                            "settings {tab:?} widened {width} pt dock to {:?}",
                            ui.min_rect()
                        );
                    },
                );
                assert!(!output.shapes.is_empty());
            }
        }
    }
}

/// What the app knows about the sync session, handed in so this window stays state-free.
pub struct SyncView<'a> {
    pub signed_in: bool,
    pub status: &'a str,
    /// The Google URL to visit, while a sign-in is in flight.
    pub login_url: Option<&'a str>,
    /// Unix seconds of the last successful sync (0 = never).
    pub last_sync: i64,
}

/// What the user asked for in the Sync tab.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SyncAction {
    SignIn,
    SignOut,
    SyncNow,
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    tab: Tab,
    prev_open: bool,
    /// Cached `.pal` file stems in the color-tables folder; rescanned on window/tab open.
    pal_stems: Vec<String>,
    scanned: bool,
    /// Hotkeys tab: the action whose key we're waiting on, the filter box, and the last conflict
    /// we resolved by stealing (shown inline so the theft isn't silent).
    rebinding: Option<BindableAction>,
    hotkey_query: String,
    stolen: Option<String>,
    /// Cache sizes, measured once on a background thread when the tab opens — a walk of a full
    /// tile cache is tens of thousands of `stat` calls and has no business on the UI thread.
    #[cfg(not(target_arch = "wasm32"))]
    storage: Option<std::sync::mpsc::Receiver<Vec<crate::storage::Entry>>>,
    #[cfg(not(target_arch = "wasm32"))]
    storage_rows: Option<Vec<crate::storage::Entry>>,
    /// Set by the General tab's two buttons; the app drains them after `show`.
    pub run_setup: bool,
    pub run_tour: bool,
    /// True while a keypress is being captured — the app stands its global hotkey table down so
    /// binding `A` doesn't also toggle the alert panel.
    pub capturing: bool,
}

impl SettingsWindow {
    pub(crate) fn prepare(&mut self) {
        if self.open && !self.prev_open {
            self.scanned = false;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.storage_rows = None;
            }
            #[cfg(target_arch = "wasm32")]
            crate::webcache::spawn_refresh_auto_cache();
        }
        self.prev_open = self.open;
        if !self.open {
            self.rebinding = None;
            self.capturing = false;
        }
    }

    /// `palettes` is read-only here (for parse-error badges); edits go through `settings` and
    /// the app reloads tables via the settings dirty-diff.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut Settings,
        palettes: &Palettes,
        sync: SyncView,
        entries: &[PaletteEntry],
        drawer: &mut crate::ui::drawer::Drawer,
    ) -> Option<SyncAction> {
        self.prepare();

        let mut open = self.open;
        let Some(window) = drawer.page(
            ctx,
            "Settings",
            &mut open,
            false,
            egui::Window::new("Settings"),
        ) else {
            self.open = open;
            return None;
        };
        let mut action = None;
        window.show(ctx, |ui| {
            // Keep the whole window inside a phone screen; tabs wrap instead of clipping.
            if cfg!(target_os = "android") {
                ui.set_max_width(ui.ctx().content_rect().width() - 28.0);
            }
            action = self.show_body(ui, settings, palettes, sync, entries, false);
        });
        self.open = open;
        if !open {
            self.prepare();
        }
        action
    }

    /// The same settings editor inside a workstation tool window or the legacy drawer.
    /// Workstation section navigation stays above its independently scrolling content.
    pub(crate) fn show_body(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        palettes: &Palettes,
        sync: SyncView,
        entries: &[PaletteEntry],
        workstation: bool,
    ) -> Option<SyncAction> {
        let tabs = [
            (Tab::General, "General"),
            (Tab::Appearance, "Appearance"),
            (Tab::Palettes, "Palettes"),
            (Tab::Units, "Units"),
            (Tab::Basemaps, "Basemaps"),
            (Tab::Alerts, "Alerts"),
            (Tab::Hotkeys, "Hotkeys"),
            (Tab::Sync, "Sync"),
            (Tab::Storage, "Storage"),
        ];
        if workstation && ui.available_width() < 540.0 {
            ui.horizontal_wrapped(|ui| {
                ui.label("Section");
                egui::ComboBox::from_id_salt("settings_section")
                    .selected_text(tabs.iter().find(|(tab, _)| *tab == self.tab).unwrap().1)
                    .width((ui.available_width() - 64.0).clamp(100.0, 240.0))
                    .show_ui(ui, |ui| {
                        for (tab, label) in tabs {
                            ui.selectable_value(&mut self.tab, tab, label);
                        }
                    });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in tabs {
                    // Chips on the phone: a `selectable_value` is a text-height target, and
                    // seven of them wrapped across a 360pt screen is a game of darts.
                    if cfg!(target_os = "android") {
                        if crate::ui::m3::chip(ui, label, self.tab == tab).clicked() {
                            self.tab = tab;
                        }
                    } else {
                        // The dock theme is a compact tool UI, not a row of soft navigation
                        // chips. Keep its Settings tabs flat, square and visibly selected.
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(label).monospace())
                                    .selected(self.tab == tab)
                                    .corner_radius(0)
                                    .min_size(egui::vec2(72.0, 24.0)),
                            )
                            .clicked()
                        {
                            self.tab = tab;
                        }
                    }
                }
            });
        }
        ui.separator();
        let action = if workstation {
            egui::ScrollArea::vertical()
                .id_salt(("settings_body", self.tab as u8))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    self.tab_contents(ui, settings, palettes, &sync, entries)
                })
                .inner
        } else {
            self.tab_contents(ui, settings, palettes, &sync, entries)
        };
        self.capturing = self.rebinding.is_some() && self.open && self.tab == Tab::Hotkeys;
        action
    }

    fn tab_contents(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        palettes: &Palettes,
        sync: &SyncView,
        entries: &[PaletteEntry],
    ) -> Option<SyncAction> {
        match self.tab {
            Tab::General => general_tab(ui, settings, &mut self.run_setup, &mut self.run_tour),
            Tab::Appearance => appearance_tab(ui, settings),
            Tab::Palettes => self.palettes_tab(ui, settings, palettes),
            Tab::Units => units_tab(ui, settings),
            Tab::Basemaps => basemaps_tab(ui, settings),
            Tab::Alerts => alerts_tab(ui, settings),
            Tab::Hotkeys => self.hotkeys_tab(ui, settings, entries),
            Tab::Sync => return sync_tab(ui, settings, sync),
            Tab::Storage => self.storage_tab(ui, settings),
        }
        None
    }

    /// What the app has written to disk, and the buttons that take it back.
    #[cfg(not(target_arch = "wasm32"))]
    fn storage_tab(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        // Kick off one measurement per tab visit; the walk lands over a channel a frame or two
        // later, and the rows stay put until Refresh or a Clear asks for a fresh one.
        if self.storage.is_none() && self.storage_rows.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::storage::report());
            });
            self.storage = Some(rx);
        }
        if let Some(rx) = &self.storage {
            if let Ok(rows) = rx.try_recv() {
                self.storage_rows = Some(rows);
                self.storage = None;
            }
        }

        let Some(rows) = &self.storage_rows else {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.weak("Measuring…");
            });
            ui.ctx().request_repaint();
            return;
        };

        let total: u64 = rows.iter().map(|r| r.bytes).sum();
        let mut recheck = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{} in caches", crate::storage::human(total)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                recheck |= ui.button("Refresh").clicked();
            });
        });
        ui.weak("Everything here is re-downloadable. Clearing costs the next fetch, nothing else.");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in rows {
                ui.horizontal_wrapped(|ui| {
                    ui.label(row.label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Only the capped directories get a Clear: the loose-files row is a
                        // mixture of things with different owners (the alert snapshot is read at
                        // startup), and deleting it wholesale is not a button's decision.
                        if row.cap.is_some() {
                            if ui.button("Clear").clicked() {
                                if let Err(e) = crate::storage::clear(&row.path) {
                                    log::warn!("clearing {}: {e}", row.path.display());
                                }
                                recheck = true;
                            }
                            if ui.button("Open").clicked() {
                                let _ = crate::platform::open_url(&row.path.to_string_lossy());
                            }
                        }
                        match row.cap {
                            Some(cap) => ui.weak(format!(
                                "{} of {}",
                                crate::storage::human(row.bytes),
                                crate::storage::human(cap)
                            )),
                            None => ui.weak(crate::storage::human(row.bytes)),
                        };
                    });
                });
            }
        });
        if recheck {
            self.storage_rows = None;
        }

        // The caps themselves. The sweep runs at startup (deleting mid-session would race the
        // fetch tasks writing into the same directories), so a change lands on the next launch.
        ui.separator();
        ui.label(egui::RichText::new("Limits").small().strong());
        for (label, mb, hint) in [
            (
                "Radar volumes",
                &mut settings.volume_cache_mb,
                "0 uses the platform default: 2 GB on the desktop, 300 MB on Android.",
            ),
            (
                "Map tiles (each cache)",
                &mut settings.tile_disk_cache_mb,
                "Applies to the raster and vector tile caches separately. 0 = platform default.",
            ),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                ui.add(
                    egui::DragValue::new(mb)
                        .range(0..=64_000)
                        .speed(50.0)
                        .suffix(" MB"),
                )
                .on_hover_text(hint);
                if *mb == 0 {
                    ui.weak("default");
                }
            });
        }
        ui.weak("New limits apply at the next start.");
    }

    /// What the browser has put in IndexedDB. Offline packs already have per-pack delete in the
    /// timeline's archive popup (`\u{22ef}`), so this shows their total rather than duplicating
    /// that control here; the automatic archive-volume cache has no other UI anywhere, so it gets
    /// a real Clear button.
    #[cfg(target_arch = "wasm32")]
    fn storage_tab(&mut self, ui: &mut egui::Ui, _settings: &mut Settings) {
        let (auto_bytes, auto_count) = crate::webcache::known_auto_cache();
        let (obj_bytes, obj_count) = crate::webcache::known_object_cache();
        let packs = crate::webcache::known_packs();
        let pack_bytes: u64 = packs.iter().map(|p| p.bytes as u64).sum();

        ui.horizontal_wrapped(|ui| {
            ui.strong(format!(
                "{} in IndexedDB",
                crate::storage::human(auto_bytes + obj_bytes + pack_bytes)
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    crate::webcache::spawn_refresh_auto_cache();
                    crate::webcache::spawn_refresh_object_cache();
                }
            });
        });
        ui.weak("Everything here is re-downloadable. Clearing costs the next fetch, nothing else.");
        ui.separator();

        // Label and Clear share a row, but the byte/count readout gets its own line below rather
        // than fighting the label for the same row's width — this window is narrow enough (see
        // the Layers panel's own width fights) that a long label plus a right-aligned readout on
        // one line draws the two on top of each other instead of wrapping.
        ui.horizontal_wrapped(|ui| {
            ui.label("Model, MRMS and satellite data");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    crate::webcache::spawn_clear_object_cache();
                }
            });
        })
        .response
        .on_hover_text(
            "Model GRIB messages, MRMS grids and GOES scans already downloaded — they never change \
             once published, so a reload reads them from here",
        );
        ui.weak(format!(
            "{} \u{2014} {} object{}",
            crate::storage::human(obj_bytes),
            obj_count,
            if obj_count == 1 { "" } else { "s" }
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label("Auto-cache");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    crate::webcache::spawn_clear_auto_cache();
                }
            });
        })
        .response
        .on_hover_text(
            "Archived (never-live) volumes you've scrubbed to, kept so revisiting the same hour \
             re-reads them instead of re-fetching from the archive.",
        );
        ui.weak(format!(
            "{} \u{2014} {} volume{}",
            crate::storage::human(auto_bytes),
            auto_count,
            if auto_count == 1 { "" } else { "s" }
        ));

        ui.add_space(4.0);
        ui.label("Offline packs").on_hover_text(
            "Loops you saved for offline playback. Manage them from the timeline's archive \
                 menu (\u{22ef}).",
        );
        ui.weak(format!(
            "{} \u{2014} {} pack{}",
            crate::storage::human(pack_bytes),
            packs.len(),
            if packs.len() == 1 { "" } else { "s" }
        ));
    }

    /// Rebindable keyboard shortcuts. Rows come from the binding table itself, so anything the
    /// command registry can do is bindable; `Palette` rows borrow the registry's own label.
    fn hotkeys_tab(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        entries: &[PaletteEntry],
    ) {
        let name = |a: BindableAction| -> String {
            match a {
                BindableAction::Palette(p) => entries
                    .iter()
                    .find(|e| e.action == p)
                    .map(|e| e.label.clone())
                    .unwrap_or_else(|| format!("{p:?}")),
                other => hotkeys::label(other).unwrap_or("").to_string(),
            }
        };

        ui.horizontal_wrapped(|ui| {
            ui.label("Filter");
            ui.add(
                egui::TextEdit::singleline(&mut self.hotkey_query)
                    .desired_width(ui.available_width().min(220.0)),
            );
            if ui.button("Reset to defaults").clicked() {
                settings.keybinds.clear();
                self.rebinding = None;
                self.stolen = None;
            }
        });
        ui.label(
            egui::RichText::new(
                "Click a shortcut, then press the key you want. Escape cancels — it always closes \
                 whatever is in front of you, so it can't be bound.",
            )
            .weak()
            .small(),
        );
        // Does the keyboard reach the app at all? The last thing seen, whether or not it is bound.
        // On a tablet this is the quickest way to tell a key the OS swallows from one the app
        // does not bind.
        {
            let last = hotkeys::last_input();
            ui.label(
                egui::RichText::new(if last.is_empty() {
                    "Keyboard test: press any key.".to_string()
                } else {
                    format!("Keyboard test: last input was {last}")
                })
                .small(),
            );
        }
        ui.separator();

        // First edit copies the whole shipped table into settings, so a later change to the
        // defaults doesn't silently rewrite keys the user already learned.
        if settings.keybinds.is_empty() {
            settings.keybinds = hotkeys::defaults();
        }

        // Capture: the first real keypress while a row is armed becomes its binding.
        if let Some(target) = self.rebinding {
            if let Some((mods, key)) = ui.ctx().input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if *key != egui::Key::Escape => Some((*modifiers, *key)),
                    _ => None,
                })
            }) {
                let shortcut = egui::KeyboardShortcut::new(mods, key);
                self.stolen =
                    hotkeys::conflict(&settings.keybinds, shortcut, target).map(|b| name(b.action));
                settings.keybinds.retain(|b| b.shortcut != shortcut);
                if let Some(row) = settings.keybinds.iter_mut().find(|b| b.action == target) {
                    row.shortcut = shortcut;
                } else {
                    settings.keybinds.push(Binding {
                        shortcut,
                        action: target,
                    });
                }
                self.rebinding = None;
            } else if ui
                .ctx()
                .input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.any_click())
            {
                self.rebinding = None;
            }
        }

        if let Some(lost) = &self.stolen {
            ui.colored_label(
                egui::Color32::from_rgb(230, 170, 60),
                format!("“{lost}” lost that key and is now unbound."),
            );
        }

        let rows: Vec<(usize, String)> = settings
            .keybinds
            .iter()
            .enumerate()
            .map(|(i, b)| (i, name(b.action)))
            .filter(|(_, label)| {
                crate::ui::layers_panel::fuzzy(&self.hotkey_query, label).is_some()
            })
            .collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, label) in rows {
                let armed = self.rebinding == Some(settings.keybinds[i].action);
                let text = if armed {
                    "press a key…".to_string()
                } else {
                    ui.ctx().format_shortcut(&settings.keybinds[i].shortcut)
                };
                ui.vertical(|ui| {
                    ui.label(label);
                    ui.horizontal_wrapped(|ui| {
                        if ui.selectable_label(armed, text).clicked() {
                            self.rebinding = if armed {
                                None
                            } else {
                                self.stolen = None;
                                Some(settings.keybinds[i].action)
                            };
                        }
                    });
                });
            }
        });
    }

    fn rescan(&mut self) {
        self.pal_stems.clear();
        if let Some(dir) = Settings::colortables_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    // File names, not stems: `.pal` and `.pal3` both load, and two tables can
                    // share a stem.
                    if p.extension().is_some_and(|x| {
                        x.eq_ignore_ascii_case("pal") || x.eq_ignore_ascii_case("pal3")
                    }) {
                        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                            self.pal_stems.push(name.to_string());
                        }
                    }
                }
            }
        }
        self.pal_stems.sort();
        self.scanned = true;
    }

    fn palettes_tab(&mut self, ui: &mut egui::Ui, settings: &mut Settings, palettes: &Palettes) {
        // Collected up front: the combo below writes to `settings` while this is being read.
        let imported: Vec<String> = settings.web_files.keys().cloned().collect();
        if !self.scanned {
            self.rescan();
        }
        let dir = Settings::colortables_dir();
        ui.horizontal_wrapped(|ui| {
            ui.label("Color tables (GRLevelX .pal)");
            if ui.button("⟳ Rescan folder").clicked() {
                self.scanned = false;
            }
        });
        if let Some(d) = &dir {
            ui.weak(format!("folder: {}", d.display()));
        }
        ui.add_space(4.0);

        egui::Grid::new("palette_grid")
            .num_columns(3)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                for moment in Moment::ALL {
                    let key = moment.short_name();
                    ui.label(key);

                    let current = settings.palettes.get(key).cloned();
                    // A `builtin:` value names a compiled-in alternate, not a file, so it has no
                    // stem — show the alternate's own name.
                    let builtin = current
                        .as_deref()
                        .and_then(|v| v.strip_prefix(crate::colormap::BUILTIN_PREFIX))
                        .map(str::to_string);
                    let current_stem = current
                        .as_deref()
                        .filter(|_| builtin.is_none())
                        .and_then(|p| std::path::Path::new(p).file_name().and_then(|s| s.to_str()))
                        .map(str::to_string);
                    let selected_text = builtin
                        .clone()
                        .or_else(|| current_stem.clone())
                        .unwrap_or_else(|| "Default".to_string());

                    egui::ComboBox::from_id_salt(("pal_combo", key))
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(current.is_none(), "Default").clicked() {
                                settings.palettes.remove(key);
                            }
                            for name in crate::colormap::alt_names(moment) {
                                let is_sel = builtin.as_deref() == Some(name);
                                if ui.selectable_label(is_sel, name).clicked() {
                                    settings.palettes.insert(
                                        key.to_string(),
                                        format!("{}{name}", crate::colormap::BUILTIN_PREFIX),
                                    );
                                }
                            }
                            for stem in &self.pal_stems {
                                let is_sel = current_stem.as_deref() == Some(stem.as_str());
                                if ui.selectable_label(is_sel, stem).clicked() {
                                    if let Some(d) = &dir {
                                        let path = d.join(stem);
                                        settings.palettes.insert(
                                            key.to_string(),
                                            path.to_string_lossy().into_owned(),
                                        );
                                    }
                                }
                            }
                            // Tables that live in the settings rather than on disk (the browser
                            // has nowhere to put a file). Named, not pathed — the name is the
                            // whole handle.
                            for name in &imported {
                                let is_sel = current.as_deref() == Some(name.as_str());
                                if ui.selectable_label(is_sel, name).clicked() {
                                    settings.palettes.insert(key.to_string(), name.clone());
                                }
                            }
                        });

                    if ui.button("Browse…").clicked() {
                        // Tagged with the moment key: the picker answers later (always, on
                        // Android), and by then nothing else remembers which row asked.
                        crate::dialog::request_open(crate::dialog::ImportKind::Palette, key);
                    }
                    ui.end_row();

                    if let Some(err) = &palettes.errors[moment.index()] {
                        ui.label("");
                        ui.colored_label(egui::Color32::from_rgb(230, 170, 80), format!("⚠ {err}"));
                        ui.label("");
                        ui.end_row();
                    }
                }
            });
    }
}

fn units_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let stacked = ui.available_width() < 440.0;
    settings_form(ui, "units_grid", |ui| {
        grid_label(ui, stacked, "Velocity / spectrum width");
        ui.horizontal_wrapped(|ui| {
            for u in VelocityUnit::ALL {
                ui.selectable_value(&mut settings.velocity_unit, u, u.label());
            }
        });
        ui.end_row();

        grid_label(ui, stacked, "Temperature");
        ui.horizontal_wrapped(|ui| {
            for u in crate::settings::TempUnit::ALL {
                ui.selectable_value(&mut settings.temp_unit, u, u.label());
            }
        })
        .response
        .on_hover_text("Surface station plots (observations arrive in Celsius)");
        ui.end_row();

        grid_label(ui, stacked, "Time display");
        ui.horizontal_wrapped(|ui| {
            for d in TimeDisplay::ALL {
                ui.selectable_value(&mut settings.time_display, d, d.label());
            }
        })
        .response
        .on_hover_text(
            "Site local reads the clock the radar is standing in; UTC is the Zulu time on the wire",
        );
        ui.end_row();

        grid_label(ui, stacked, "Layer time warning (min)");
        ui.add(egui::DragValue::new(&mut settings.time_mismatch_minutes).range(0..=120))
            .on_hover_text("Warn when a layer's valid time differs from the radar scan on screen by more than this many minutes");
        ui.end_row();
    });
    ui.weak("Reflectivity stays dBZ; internal data is unchanged (display-only).");
}

fn grid_label(ui: &mut egui::Ui, stacked: bool, label: &str) {
    ui.label(label);
    if stacked {
        ui.end_row();
    }
}

fn settings_form(ui: &mut egui::Ui, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    if ui.available_width() < 440.0 {
        ui.vertical(contents);
    } else {
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, contents);
    }
}

/// The "Custom (XYZ URL)" basemap's template, zoom cap and attribution.
///
/// Hidden on web: the tile proxy is an exact-host allowlist, so an arbitrary host cannot be
/// fetched there at all (see `BasemapStyle::available`).
fn custom_tile_source(ui: &mut egui::Ui, settings: &mut Settings) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    ui.separator();
    ui.label("Custom tile source");
    ui.weak(
        "An XYZ template adds a \"Custom\" entry to the basemap list. Desktop and Android only.",
    );
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("URL template");
        ui.add(
            egui::TextEdit::singleline(&mut settings.custom_tile_url)
                .hint_text("https://tiles.example.com/{z}/{x}/{y}.png")
                .desired_width(ui.available_width().clamp(80.0, 340.0)),
        );
    });
    if !settings.custom_tile_url.is_empty()
        && !crate::tiles::valid_xyz_template(&settings.custom_tile_url)
    {
        // Says why rather than silently dropping the entry from the list.
        ui.colored_label(
            egui::Color32::from_rgb(220, 120, 100),
            "Needs to be an https URL containing {z}, {x} and {y}.",
        );
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Max zoom");
        ui.add(egui::DragValue::new(&mut settings.custom_tile_max_z).range(1..=22))
            .on_hover_text(
                "Deeper views stretch the deepest tile that loaded rather than blanking",
            );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Attribution");
        ui.add(
            egui::TextEdit::singleline(&mut settings.custom_tile_attribution)
                .hint_text("© Your data source")
                .desired_width(ui.available_width().clamp(80.0, 340.0)),
        );
    });
}

/// ROADMAP_NEW B6.11 step 11's Advanced-settings surface: the self-hosted `radar-ingest` relay URL
/// (B6's second, independently-acquired live Level II path) and the manual failover override
/// (B6.9's "preserve a manual provider override in Advanced settings for diagnostics"). Native
/// only — `crate::radar_provider_manager` doesn't build on wasm32, and neither does the relay
/// client it would configure.
fn radar_relay_section(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.strong("Radar relay (advanced)");
    ui.weak(
        "Optional second live Level II source: your own self-hosted radar-ingest relay, run \
         alongside the built-in Unidata/AWS feed for redundancy. Never a hosted HookEcho service \
         \u{2014} you point this at infrastructure you or someone you trust runs.",
    );
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("Relay URL");
        ui.add(
            egui::TextEdit::singleline(&mut settings.radar_relay_url)
                .hint_text("https://relay.example.com")
                .desired_width(ui.available_width().clamp(80.0, 300.0)),
        );
    });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("Force data source (testing)");
        egui::ComboBox::from_id_salt("radar_provider_override")
            .selected_text(settings.radar_provider_override.label())
            .show_ui(ui, |ui| {
                for opt in crate::settings::RadarProviderOverride::ALL {
                    ui.selectable_value(&mut settings.radar_provider_override, opt, opt.label());
                }
            });
    });
    if settings.radar_provider_override != crate::settings::RadarProviderOverride::Auto {
        ui.colored_label(
            egui::Color32::YELLOW,
            "\u{26a0} Forcing a source overrides automatic failover until set back to Auto.",
        );
    }
    ui.weak(
        "For testing: pin live radar to one specific path regardless of health, to check that a \
         source actually works or to reproduce a problem on it deliberately. Auto (default) lets \
         the failover arbiter pick automatically.",
    );
}

fn basemaps_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label("Provider API keys unlock additional raster basemap styles.");
    ui.add_space(6.0);
    key_field(ui, "Mapbox access token", &mut settings.mapbox_key);
    ui.add_space(8.0);
    key_field(ui, "MapTiler API key", &mut settings.maptiler_key);
    ui.add_space(6.0);
    ui.weak("Keys are stored locally in settings.json and sent only to the provider's tile API.");
    ui.add_space(12.0);
    custom_tile_source(ui, settings);
    ui.add_space(12.0);
    ui.separator();
    ui.label("Live station cards");
    ui.weak("Optional. Airport METARs need no key; these add personal weather stations.");
    ui.add_space(6.0);
    key_field(ui, "WeatherFlow Tempest token", &mut settings.tempest_token);
    ui.add_space(8.0);
    key_field(ui, "Weather Underground API key", &mut settings.wu_key);
    ui.add_space(8.0);
    key_field(ui, "Synoptic Data token", &mut settings.synoptic_token);
    ui.weak(
        "Synoptic aggregates the mesonets \u{2014} state, university and highway-department \
         networks. The stations actually inside the storm, rather than at the airport.",
    );
    ui.add_space(12.0);
    ui.separator();
    ui.label("Crowd reports");
    ui.weak(
        "Optional. A free mPING key (mping.ou.edu) adds crowd-sourced precipitation-type \
         reports \u{2014} the only source that says whether it is landing as rain or snow.",
    );
    ui.add_space(6.0);
    key_field(ui, "mPING API key", &mut settings.mping_key);
    ui.add_space(12.0);
    ui.separator();
    ui.label("Webcams");
    ui.weak(
        "Optional. The FAA's ~2,600 cameras need no key but stop at the US border; a free Windy \
         key adds their global network. Windy returns the 50 most popular cameras in view.",
    );
    ui.add_space(6.0);
    key_field(ui, "Windy API key", &mut settings.windy_key);
    ui.add_space(12.0);
    ui.separator();
    ui.label("Air quality");
    ui.weak("Optional. A free key from docs.airnowapi.org turns on the AirNow AQI layer.");
    ui.add_space(6.0);
    key_field(ui, "AirNow API key", &mut settings.airnow_key);
    ui.add_space(8.0);
    ui.label("Field mill URL (JSON, kV/m)");
    ui.text_edit_singleline(&mut settings.field_mill_url)
        .on_hover_text(
            "A ground field mill publishing {\"time\": …, \"kv_per_m\": …}. Left empty, the cards \
             chart NOAA's ionospheric PPEF model in mV/m instead — a different quantity entirely.",
        );
}

/// A masked API-key entry: a label above a field that fills the row, then a Clear (✕) button, and
/// on Android a Paste button that fills the field straight from the system clipboard (typing long
/// keys on a soft keyboard is impractical, and reading the clipboard directly sidesteps IME
/// quirks). Laid out responsively so it fits any width, phone included.
pub(crate) fn key_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        // Reserve space for the trailing buttons; the field takes the rest.
        let paste_w = if cfg!(target_os = "android") {
            62.0
        } else {
            0.0
        };
        let field_w = (ui.available_width() - paste_w - 34.0).max(80.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .password(true)
                .desired_width(field_w),
        );
        #[cfg(target_os = "android")]
        if ui
            .button("Paste")
            .on_hover_text("Paste from clipboard")
            .clicked()
        {
            if let Some(t) = crate::platform::clipboard_text() {
                *value = t.trim().to_string();
            }
        }
        // Phosphor X, not "✕": the Android fallback face has no U+2715 and drew a tofu box.
        if !value.is_empty()
            && ui
                .small_button(egui_phosphor::regular::X)
                .named("Clear")
                .clicked()
        {
            value.clear();
        }
    });
}

/// Sign in with Google and keep every machine's settings the same. The data goes to the hidden
/// per-app folder in the user's own Drive — there is no HookEcho account and no server.
fn sync_tab(ui: &mut egui::Ui, settings: &mut Settings, sync: &SyncView) -> Option<SyncAction> {
    let mut action = None;
    ui.label("Sign in with Google to keep your settings, saved locations, placefiles and API keys the same on every machine.");
    ui.add_space(6.0);
    if sync.signed_in {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("✓ Signed in").strong());
            if ui.button("Sign out").clicked() {
                action = Some(SyncAction::SignOut);
            }
        });
        ui.checkbox(
            &mut settings.sync_enabled,
            "Sync automatically (every 5 minutes)",
        );
        if ui.button("Sync now").clicked() {
            action = Some(SyncAction::SyncNow);
        }
        if sync.last_sync > 0 {
            let ago = (crate::share::now() - sync.last_sync).max(0);
            ui.weak(match ago {
                0..=59 => "last synced just now".to_string(),
                60..=3599 => format!("last synced {} min ago", ago / 60),
                _ => format!("last synced {} h ago", ago / 3600),
            });
        }
    } else if let Some(url) = sync.login_url {
        // The browser has the user right now. Show the link anyway: on a phone (or a desktop with
        // no opener) launching it can fail, and then this is the only way through.
        ui.label("Waiting for you to finish in the browser…");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open sign-in page").clicked() {
                if let Err(e) = crate::platform::open_url(url) {
                    log::warn!("open_url failed: {e}");
                }
            }
            if ui.button("Copy link").clicked() {
                ui.ctx().copy_text(url.to_string());
            }
        });
    } else {
        ui.label("Your own Google OAuth client (one-time setup — see docs/sync.md):");
        key_field(ui, "Client ID", &mut settings.sync_client_id);
        ui.add_space(4.0);
        key_field(ui, "Client secret", &mut settings.sync_client_secret);
        ui.add_space(6.0);
        if ui.button("Sign in with Google").clicked() {
            action = Some(SyncAction::SignIn);
        }
    }
    if !sync.status.is_empty() {
        ui.add_space(6.0);
        ui.weak(sync.status);
    }
    ui.add_space(6.0);
    ui.weak(
        "Screen scale, device name and background alerts stay local to each machine; everything \
         else follows the sync. The grant covers only this app's own Drive folder.",
    );
    action
}

/// theme_plan.md §3: split out of `general_tab` — everything that changes how the app *looks*
/// (colors, chrome, density, the timeline's own visual style) rather than how it *behaves*.
/// `Theme` (this app's own chrome+color preset — the type is `Layout`, see that type's own doc
/// comment for the naming story) reads as the primary choice here; `Color scheme` (just colors —
/// the type is `Theme`) is the secondary, customize-further control underneath it, so a casual
/// user picks one Theme and is done, while a power user can still override just the colors.
/// There is one colour scheme (the Dock's Dear ImGui look), so only its accent is a choice here.
fn appearance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let stacked = ui.available_width() < 440.0;
    settings_form(ui, "appearance_grid", |ui| {
        // A phone picks its own design below; a tablet, being the desktop layout, picks a theme
        // here like a desktop does.
        if crate::platform::phone_layout() {
            grid_label(ui, stacked, "Phone design");
            ui.vertical(|ui| {
                for d in crate::settings::PhoneDesign::ALL {
                    ui.selectable_value(&mut settings.phone_design, d, d.label())
                        .on_hover_text(d.tagline());
                }
                ui.small(settings.phone_design.tagline());
            });
            ui.end_row();
        } else {
            grid_label(ui, stacked, "Theme");
            ui.horizontal_wrapped(|ui| {
                for l in crate::settings::Layout::ALL {
                    ui.selectable_value(&mut settings.layout, l, l.label());
                }
            })
            .response
            .on_hover_text(
                "Command Ribbon: HookEcho's original docked toolbar. WSV3: the analyst \
                     workstation, map-first \u{2014} a two-row top bar and a docked timeline, \
                     with the Layers and Inspector windows opened when you want them. Minimal: \
                     the original map-first search pill and control column. Dock (ImGui): the \
                     workstation with its windows showing.",
            );
            ui.end_row();
        }

        grid_label(ui, stacked, "Accent color");
        ui.horizontal_wrapped(|ui| {
            let mut on = settings.accent.is_some();
            let theme_accent = crate::theme::accent(settings.theme);
            if ui.checkbox(&mut on, "Custom").changed() {
                settings.accent =
                    on.then(|| [theme_accent.r(), theme_accent.g(), theme_accent.b()]);
            }
            if let Some(rgb) = settings.accent.as_mut() {
                ui.color_edit_button_srgb(rgb);
            }
        });
        ui.end_row();

        grid_label(ui, stacked, "Timeline");
        ui.horizontal_wrapped(|ui| {
            for s in crate::settings::TimelineStyle::ALL {
                ui.selectable_value(&mut settings.timeline_style, s, s.label());
            }
        })
        .response
        .on_hover_text(
            "Default: the floating scrub-track pill. WSV3: an explicit transport row, a \
                 loop-length control, and a plain slider. Compact: a slim single-row strip.",
        );
        ui.end_row();

        if !cfg!(target_os = "android") {
            grid_label(ui, stacked, "Floating search");
            ui.checkbox(&mut settings.floating_search_button, "Floating icon button")
                .on_hover_text(
                    "Replace the ribbon's docked \"Search\" group with a small floating icon \
                         button over the map instead. Only affects the ribbon themes (Command \
                         Ribbon / WSV3) — the Minimal layout's own search pill already goes \
                         icon-only on a narrow window.",
                );
            ui.end_row();
        }
    });
}

fn general_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    run_setup: &mut bool,
    run_tour: &mut bool,
) {
    let stacked = ui.available_width() < 440.0;
    settings_form(ui, "general_grid", |ui| {
        grid_label(ui, stacked, "Default site");
        let mut site = settings.default_site.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut site)
                    .desired_width(ui.available_width().min(240.0)),
            )
            .changed()
        {
            settings.default_site = site.to_ascii_uppercase();
        }
        ui.end_row();

        grid_label(ui, stacked, "Poll interval (s)");
        ui.add(egui::DragValue::new(&mut settings.poll_interval_secs).range(10..=600));
        ui.end_row();

        grid_label(ui, stacked, "Motion");
        ui.checkbox(&mut settings.reduce_motion, "Reduce motion")
            .on_hover_text(
                "Panels and cards appear where they belong instead of sliding in. The app \
                     also turns this on for itself if frames get slow.",
            );
        ui.end_row();

        grid_label(ui, stacked, "3D map");
        ui.vertical(|ui| {
            ui.checkbox(&mut settings.hide_far_3d, "Hide far-away items")
                .on_hover_text(
                    "When the map is tilted, stop drawing storm reports, lightning, sites and \
                         other markers far out toward the horizon, where they pile up and float \
                         above the map. The radar and map themselves are always drawn.",
                );
            ui.add_enabled(
                settings.hide_far_3d,
                egui::Slider::new(&mut settings.far_3d_factor, 1.2..=6.0)
                    .text("Distance")
                    .custom_formatter(|v, _| format!("{v:.1}x")),
            )
            .on_hover_text(
                "How far past the centre of the map, as a multiple of the camera's distance \
                     to it. Lower hides more.",
            );
        });
        ui.end_row();

        grid_label(ui, stacked, "UI scale");
        // Phones start denser: 0.5 × a 4.0 density factor ≈ a desktop-density canvas.
        let lo = if cfg!(target_os = "android") {
            0.5
        } else {
            0.7
        };
        crate::theme::slider(ui, egui::Slider::new(&mut settings.ui_scale, lo..=1.6).step_by(0.05));
        ui.end_row();
    });
    ui.weak(
        "UI scale also responds to Ctrl+= / Ctrl+- / Ctrl+0. Colors, chrome and the \
             timeline live under the Appearance tab now.",
    );

    let valid = wxdata::sites::site_by_id(&settings.default_site).is_some();
    if !valid && !settings.default_site.is_empty() {
        ui.colored_label(egui::Color32::YELLOW, "⚠ unknown site id");
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Getting started");
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Set up again\u{2026}")
            .on_hover_text("Pick your home radar again")
            .clicked()
        {
            *run_setup = true;
        }
        if ui
            .button("Take the tour\u{2026}")
            .on_hover_text("A 60-second walk through the app's controls")
            .clicked()
        {
            *run_tour = true;
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Background");
    ui.checkbox(
        &mut settings.close_to_tray,
        "Keep running in background when window closes",
    )
    .on_hover_text(
        "Closing the window minimizes instead of quitting, so alert polling + push keep going",
    );

    if !cfg!(target_arch = "wasm32") {
        ui.add_space(8.0);
        ui.separator();
        radar_relay_section(ui, settings);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Diagnostics");
    if ui
        .checkbox(&mut settings.analyst_mode, "Analyst mode")
        .on_hover_text(
            "Raises the app's own log verbosity and opens a live filtered log of radar/provider \
             detail that doesn't show up at the normal level \u{2014} sweep-by-sweep chunk \
             arrival, tilt/VCP progress, and (when a relay is configured) provider health and \
             failover transitions. Costs real log volume while on; negligible when off.",
        )
        .changed()
    {
        crate::devlog::set_analyst_mode(settings.analyst_mode);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Workspaces");
    if settings.workspaces.is_empty() {
        ui.weak("None yet \u{2014} arrange your panes, then run \"Save workspace\" from Ctrl+K.");
    } else {
        ui.weak("Restore one from Ctrl+K. Renaming here is what the command is called.");
    }
    let mut remove = None;
    for (i, ws) in settings.workspaces.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut ws.name).desired_width(220.0));
            ui.weak(format!(
                "{} pane{}",
                ws.panes.len(),
                if ws.panes.len() == 1 { "" } else { "s" }
            ));
            if ui.button("Delete").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        settings.workspaces.remove(i);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("AI");
    ui.horizontal_wrapped(|ui| {
        ui.label("Storm Digest written by:");
        egui::ComboBox::from_id_salt("ai_provider")
            .selected_text(settings.ai_provider.label())
            .show_ui(ui, |ui| {
                for p in crate::digest::Provider::ALL {
                    ui.selectable_value(&mut settings.ai_provider, p, p.label());
                }
            });
    });
    match settings.ai_provider {
        crate::digest::Provider::Anthropic => {
            ui.horizontal_wrapped(|ui| {
                ui.label("Anthropic key:");
                ui.add(
                    egui::TextEdit::singleline(&mut settings.anthropic_key)
                        .password(true)
                        .hint_text("sk-ant-…")
                        .desired_width(240.0),
                );
            });
        }
        crate::digest::Provider::Gemini => {
            ui.horizontal_wrapped(|ui| {
                ui.label("Google AI Studio key:");
                ui.add(
                    egui::TextEdit::singleline(&mut settings.gemini_key)
                        .password(true)
                        .hint_text("AIza…")
                        .desired_width(240.0),
                );
            });
            ui.hyperlink_to(
                "Get a key at aistudio.google.com",
                "https://aistudio.google.com/apikey",
            );
        }
    }
    ui.weak("Optional. Storm Digest (Ctrl+K) works offline; a key lets the chosen model write friendlier prose. Keys are held locally only.");
}

/// Alert-sound controls: master toggle, volume, and a per-event sound picker with previews.
/// Shared by the Settings ▸ Audio tab and the alert rules page.
pub fn sound_picker(ui: &mut egui::Ui, settings: &mut Settings) {
    use crate::settings::AlertSound;

    ui.checkbox(
        &mut settings.alert_spotlight,
        "Dim the map around an open alert",
    )
    .on_hover_text("While an alert's card is open, the map outside its area is darkened");
    ui.checkbox(&mut settings.yall_mode, "Y'all mode")
        .on_hover_text(
            "The Y'all-O-Meter: how worried to be at your location (or the map's middle), with the watches, outlook and storms headed your way, said plainly",
        );
    ui.checkbox(&mut settings.mute_alerts, "Mute all alert audio")
        .on_hover_text("Silences chimes and spoken warnings without changing the choices below");
    ui.checkbox(&mut settings.alert_sound, "Play a sound on alerts")
        .on_hover_text("Master switch for the warning / TDS / lightning alert sounds");
    ui.checkbox(&mut settings.scan_chime, "Chime on every new scan")
        .on_hover_text(
            "A tap on the shoulder when a new volume lands on the live pane you're watching",
        );
    ui.horizontal_wrapped(|ui| {
        ui.label("Volume");
        crate::theme::slider(ui, egui::Slider::new(&mut settings.alert_volume, 0.0..=1.0).step_by(0.05));
    });
    ui.add_space(4.0);

    // One row per alert kind: sound combo (+ Custom… file picker) and a ▶ preview.
    type SoundRow = (&'static str, fn(&mut Settings) -> &mut AlertSound);
    let rows: [SoundRow; 6] = [
        ("New scan", |s| &mut s.scan_sound),
        ("Warning", |s| &mut s.warn_sound),
        ("Emergency", |s| &mut s.emergency_sound),
        ("Tornado debris / confirmed", |s| &mut s.tds_sound),
        ("Tornado likely / rotation", |s| &mut s.rotation_sound),
        ("Lightning", |s| &mut s.lightning_sound),
    ];
    let volume = settings.alert_volume;
    let stacked = ui.available_width() < 440.0;
    settings_form(ui, "sound_grid", |ui| {
        for (label, field) in rows {
            grid_label(ui, stacked, label);
            ui.horizontal_wrapped(|ui| {
                let sound = field(settings);
                egui::ComboBox::from_id_salt(label)
                    .selected_text(sound.label())
                    .show_ui(ui, |ui| {
                        for b in AlertSound::BUILTINS {
                            let sel = sound.label() == b.label();
                            if ui.selectable_label(sel, b.label()).clicked() {
                                *sound = b;
                            }
                        }
                        let is_custom = matches!(sound, AlertSound::Custom(_));
                        // A custom sound is a file the mixer reopens on every alert, and a
                        // browser has no path that outlives the picker. Built-ins only there.
                        if !cfg!(target_arch = "wasm32")
                            && ui.selectable_label(is_custom, "Custom…").clicked()
                        {
                            crate::dialog::request_open(
                                crate::dialog::ImportKind::AlertSound,
                                label,
                            );
                        }
                    });
                let preview = sound.clone();
                if ui
                    .button(egui_phosphor::regular::PLAY)
                    .on_hover_text("Preview")
                    .clicked()
                {
                    crate::audio::play(&preview, volume);
                }
            });
            ui.end_row();
        }
    });
}

/// Everything that fires when weather happens: sounds, push, proximity alarms.
fn alerts_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    sound_picker(ui, settings);

    ui.add_space(8.0);
    ui.separator();
    ui.strong("When to interrupt");
    ui.add_enabled_ui(!cfg!(target_os = "android"), |ui| {
        let r = ui
            .checkbox(&mut settings.desktop_notify, "Post alerts to the desktop")
            .on_hover_text(
                "Use the system notification centre, so an alert arrives with the window \
                 behind something else",
            );
        // Turning it on is the moment to ask, and the only one — a browser that is asked at load,
        // or when a warning fires, gets a permission dialog nobody was expecting. No-op natively.
        if r.changed() && settings.desktop_notify {
            crate::notify::ask_permission();
        }
    });
    ui.checkbox(&mut settings.alert_follow_gps, "Alert where I am, too")
        .on_hover_text(
            "While a GPS fix is coming in, your own position joins the saved locations the \
             lightning and rotation alerts watch. Nothing is saved or shared.",
        );
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut settings.quiet_hours, "Quiet hours");
        ui.add_enabled_ui(settings.quiet_hours, |ui| {
            ui.add(egui::DragValue::new(&mut settings.quiet_start_hour).range(0..=23));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut settings.quiet_end_hour).range(0..=23));
            ui.weak("local");
        });
    });
    ui.weak(
        "Holds sounds and pushes between those hours. Tornado Emergency, PDS and destructive \
         warnings still come through — that tier is what quiet hours is for.",
    );
    ui.horizontal_wrapped(|ui| {
        ui.label("Push and sound only for:");
        for (tier, label) in [
            (0u8, "Every warning"),
            (1, "Considerable and up"),
            (2, "Escalated only"),
        ] {
            ui.selectable_value(&mut settings.alert_min_escalation, tier, label);
        }
    });
    ui.weak("Quieter warnings still banner and still show in the alert list.");
    ui.horizontal_wrapped(|ui| {
        ui.label("Roll up after");
        ui.add(
            egui::DragValue::new(&mut settings.alert_rollup_threshold)
                .range(0..=50)
                .suffix(" alerts"),
        );
        ui.label("within");
        ui.add(
            egui::DragValue::new(&mut settings.alert_rollup_window_min)
                .range(1..=120)
                .suffix(" min"),
        );
    });
    ui.weak(
        "On an outbreak day, pushes past that rate collapse into one rolling summary instead of one buzz per warning. 0 turns it off; escalated warnings always push as themselves.",
    );

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Push notifications (ntfy.sh)");
    ui.horizontal_wrapped(|ui| {
        ui.label("Topic:");
        ui.add(egui::TextEdit::singleline(&mut settings.ntfy_topic).hint_text("your-secret-topic"));
    });
    ui.weak("When a warning covers a saved location marker, a push is sent to ntfy.sh/<topic>.");
    ui.weak("Subscribe to the same topic in the ntfy app on your phone. Leave blank to disable.");
    ui.add_enabled_ui(!cfg!(target_os = "android"), |ui| {
        ui.checkbox(&mut settings.ntfy_snapshot, "Attach a picture of the radar")
            .on_hover_text(
                "Pushes the view you're looking at alongside the warning. Desktop only — the \
                 phone's background alert service has nothing to render from.",
            );
    });

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Chat webhooks");
    ui.horizontal_wrapped(|ui| {
        ui.label("Discord:");
        ui.add(
            egui::TextEdit::singleline(&mut settings.discord_webhook)
                .hint_text("https://discord.com/api/webhooks/…"),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Slack:");
        ui.add(
            egui::TextEdit::singleline(&mut settings.slack_webhook)
                .hint_text("https://hooks.slack.com/services/…"),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Matrix server:");
        ui.add(
            egui::TextEdit::singleline(&mut settings.matrix_homeserver)
                .hint_text("https://matrix.org"),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Matrix room:");
        ui.add(egui::TextEdit::singleline(&mut settings.matrix_room).hint_text("!room:matrix.org"));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Matrix token:");
        ui.add(
            egui::TextEdit::singleline(&mut settings.matrix_token)
                .password(true)
                .hint_text("access token"),
        );
    });
    ui.weak("Every alert that goes to ntfy also posts here. Blank fields are off.");
    ui.weak("These URLs and the token are secrets — they stay in your settings file.");

    // No Android (its alerts leave through the foreground service) and no web (no TCP socket).
    if !cfg!(any(target_os = "android", target_arch = "wasm32")) {
        ui.add_space(8.0);
        ui.separator();
        ui.strong("MQTT");
        ui.horizontal_wrapped(|ui| {
            ui.label("Broker:");
            ui.add(
                egui::TextEdit::singleline(&mut settings.mqtt_host)
                    .desired_width(160.0)
                    .hint_text("mqtt.lan"),
            );
            ui.add(egui::DragValue::new(&mut settings.mqtt_port).range(1..=65535));
            ui.checkbox(&mut settings.mqtt_tls, "TLS");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("User:");
            ui.add(
                egui::TextEdit::singleline(&mut settings.mqtt_user)
                    .desired_width(110.0)
                    .hint_text("optional"),
            );
            ui.label("Password:");
            ui.add(
                egui::TextEdit::singleline(&mut settings.mqtt_pass)
                    .desired_width(110.0)
                    .password(true),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Topic prefix:");
            ui.add(egui::TextEdit::singleline(&mut settings.mqtt_prefix).hint_text("home/weather"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut settings.mqtt_discovery, "Home Assistant discovery")
                .on_hover_text(
                    "Publish retained config topics so Home Assistant creates the device itself, \
                     with a mute switch that publishes back to <prefix>/cmd/mute. Leave off if \
                     this broker has no Home Assistant on it.",
                );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Strikes topic:");
            ui.add(
                egui::TextEdit::singleline(&mut settings.strikes_topic)
                    .hint_text("blitzortung/1.1/#"),
            )
            .on_hover_text(
                "Subscribe to lightning strikes someone else is already publishing to this \
                 broker \u{2014} a Home Assistant Blitzortung integration, or the relay in \
                 scripts/strikes-relay. Empty is off. HookEcho never connects to a strike \
                 network itself.",
            );
        });
        ui.weak(
            "Publishes <prefix>/status and <prefix>/nearest every five minutes, and \
             <prefix>/alerts as warnings arrive. Takes effect on restart.",
        );
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Battery");
    if ui
        .checkbox(
            &mut settings.battery_saver,
            "Battery saver — check less often",
        )
        .on_hover_text(
            "Slows every cadence the app controls: the screen redraws four times a second \
             instead of ten, volumes are polled half as often, and on Android the background \
             alert poll drops from every 5 minutes to every 15. Warnings still arrive, later.",
        )
        .changed()
    {
        crate::platform::set_battery_saver(settings.battery_saver);
    }

    if cfg!(target_os = "android") {
        ui.add_space(8.0);
        ui.separator();
        ui.strong("Background alerts");
        if ui
            .checkbox(
                &mut settings.background_alerts,
                "Watch my saved locations while the app is closed",
            )
            .on_hover_text(
                "Runs a small service that checks api.weather.gov for your marker locations and \
                 posts a notification. Tapping it flies the map to that location. Costs a \
                 permanent notification and some battery.",
            )
            .changed()
        {
            crate::platform::set_background_alerts(settings.background_alerts);
        }
        alert_health(ui);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Weather radio relays");
    ui.weak(
        "NOAA broadcasts NWR on VHF and streams nothing itself, so these are listener-run relays \
         — find the MP3 URL for your county and paste it here. Play them from the drawer.",
    );
    let mut remove = None;
    for (i, s) in settings.nwr_streams.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut s.name)
                    .hint_text("KEC55 Norman")
                    .desired_width(130.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut s.url)
                    .hint_text("https://…/stream.mp3")
                    .desired_width(220.0),
            );
            if ui.button("✖").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        settings.nwr_streams.remove(i);
    }
    if ui.button("Add relay").clicked() {
        settings.nwr_streams.push(crate::settings::NwrStream {
            name: String::new(),
            url: String::new(),
        });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Spoken warnings");
    ui.checkbox(&mut settings.speak_warnings, "Read new warnings aloud")
        .on_hover_text(
            "The tone first, then the words: which counties, the towns in the path, where it sits \
             from your saved place, and what to do \u{2014} for when your eyes are on the road",
        );
    ui.weak(
        "Piper below is the good voice; without it Linux uses spd-say or espeak, macOS and \
             Windows their own, Android its own.",
    );
    // Hearing it once beats reading three settings and waiting for weather to find out that the
    // engine was never installed.
    if ui
        .button("\u{1f50a} Speak a test warning")
        .on_hover_text("Plays the emergency tone and reads a made-up tornado warning")
        .clicked()
    {
        speak_test(settings);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !cfg!(target_os = "android") {
        piper_row(ui, settings);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Proximity alarms");
    ui.checkbox(
        &mut settings.rain_alerts,
        "Rain heading for a saved location",
    )
    .on_hover_text(
        "Watches upwind of your saved places and your chase position, and says roughly how \
             many minutes out the rain is",
    );
    ui.checkbox(&mut settings.lightning_alarm, "Lightning within ~15 km of a saved location")
        .on_hover_text("Chime + push when CG lightning strikes near a marker. Requires the Lightning layer (National) to be on.");
}

/// Speak a warning that never happened, through the whole chain the real ones use.
///
/// Deliberately the escalated path: the emergency tone and a catastrophic tornado warning, which
/// is the one worth knowing works. The home marker's name is borrowed so the relation clause
/// sounds like it will on the night.
fn speak_test(settings: &Settings) {
    let home = settings
        .markers
        .iter()
        .find(|m| m.home)
        .or_else(|| settings.markers.first())
        .map(|m| m.name.as_str())
        .unwrap_or("Home");
    crate::speech::set_volume(settings.alert_volume);
    // Imperial, because the demo is an Oklahoma tornado warning. Real ones follow the pane's
    // radar network, which Settings has no pane to ask.
    let script = wxdata::spoken::warning_script(
        &wxdata::spoken::demo_alert(),
        &wxdata::spoken::relation(home, 19.3, Some(45.0), false),
        "7 15 PM",
    );
    let tone = settings
        .alert_sound
        .then(|| (settings.emergency_sound.clone(), settings.alert_volume));
    crate::speech::announce(crate::speech::Priority::Emergency, tone, vec![script]);
}

/// Piper: the local neural voice, and the one download this app ever offers.
///
/// Nothing here is bundled. The binary is whatever the user installed (most distros package it),
/// and the voice model is a ~60 MB file that would be absurd to commit — so the button fetches it
/// into the data directory on request, and until then espeak keeps working.
#[cfg(not(target_arch = "wasm32"))]
fn piper_row(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(4.0);
    let mut edited = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("Piper binary:");
        edited |= ui
            .add(
                egui::TextEdit::singleline(&mut settings.piper_path)
                    .hint_text("blank = find `piper` on PATH"),
            )
            .changed();
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Voice model:");
        edited |= ui
            .add(
                egui::TextEdit::singleline(&mut settings.piper_voice)
                    .hint_text("path to a .onnx voice"),
            )
            .changed();
    });
    if edited {
        crate::speech::set_piper(&settings.piper_path, &settings.piper_voice);
    }
    // Only once a voice is chosen: until then Piper is off on purpose and there is nothing wrong.
    // Android speaks through its own TextToSpeech and has no Piper to diagnose — and the runtime
    // `if !cfg!(android)` at the call site still compiles this body, so the gate has to be here.
    #[cfg(not(target_os = "android"))]
    if !settings.piper_voice.is_empty() {
        if let Some(problem) = crate::speech::piper_problem() {
            ui.colored_label(ui.visuals().warn_fg_color, problem);
        }
    }
    // The picker downloads; the field above is what actually gets spoken with. Picking a voice
    // that is already on disk switches to it without touching the network.
    ui.horizontal_wrapped(|ui| {
        let status = crate::speech::voice_status();
        let mut want = settings.piper_download_voice.clone();
        if want.is_empty() {
            want = crate::speech::DEFAULT_VOICE.to_string();
        }
        egui::ComboBox::from_id_salt("piper-voice-pick")
            .selected_text(&want)
            .show_ui(ui, |ui| {
                for id in crate::speech::VOICES {
                    if ui.selectable_label(want == *id, *id).clicked() {
                        settings.piper_download_voice = (*id).to_string();
                    }
                }
            });
        let on_disk = crate::speech::voice_path(&want).filter(|p| p.exists());
        match on_disk {
            Some(p) if settings.piper_voice != p.to_string_lossy() => {
                if ui.button("Use this voice").clicked() {
                    settings.piper_voice = p.to_string_lossy().into_owned();
                    crate::speech::set_piper(&settings.piper_path, &settings.piper_voice);
                }
            }
            Some(_) => {
                ui.weak("in use");
            }
            None => {
                if !status.busy && ui.button("Download").clicked() {
                    crate::speech::request_voice_download(&want);
                }
            }
        }
        if !status.text.is_empty() {
            ui.weak(&status.text);
        }
    });
    // A finished download is only useful once something points at it, and the user asking for a
    // voice is the whole configuration step — so adopt it as soon as it lands.
    if settings.piper_voice.is_empty() {
        let want = if settings.piper_download_voice.is_empty() {
            crate::speech::DEFAULT_VOICE.to_string()
        } else {
            settings.piper_download_voice.clone()
        };
        if let Some(p) = crate::speech::voice_path(&want).filter(|p| p.exists()) {
            settings.piper_voice = p.to_string_lossy().into_owned();
            crate::speech::set_piper(&settings.piper_path, &settings.piper_voice);
        }
    }
    ui.checkbox(
        &mut settings.speak_position,
        "Speak the nearest storm's bearing while chasing",
    );
}

/// Delivery health for the Android alert stack: what the OS is currently letting us do, and the
/// two prompts that change it.
///
/// Worth its own panel because every honest answer to "why didn't I get that warning" is here,
/// and none of it is visible anywhere else on the phone. The Samsung line is not an excuse — it
/// is the truth, and a user who knows to check One UI's app-sleep list is better off than one
/// who thinks the app is broken.
fn alert_health(ui: &mut egui::Ui) {
    let Some(h) = crate::platform::alert_health() else {
        return;
    };
    let ago = |d: Option<std::time::Duration>| match d {
        Some(d) if d.as_secs() < 90 => format!("{} s", d.as_secs()),
        Some(d) => format!("{} min", d.as_secs() / 60),
        None => "never".to_string(),
    };
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("Last check:");
        ui.strong(ago(h.since_poll));
        ui.label("· next in");
        ui.strong(match h.until_next {
            Some(d) => format!("{} min", d.as_secs() / 60),
            None => "not scheduled".to_string(),
        });
    });
    if !h.exact {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(230, 160, 60),
                "\u{26a0} Inexact alarms",
            );
            if ui.button("Allow exact alarms").clicked() {
                crate::platform::request_exact_alarms();
            }
        });
        ui.weak("Without this, checks arrive on Android's own schedule — minutes late, or later.");
    }
    if !h.exempt {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(230, 160, 60),
                "\u{26a0} Battery optimised",
            );
            if ui.button("Exempt from battery optimisation").clicked() {
                crate::platform::request_battery_exemption();
            }
        });
        ui.weak("This is the one that decides whether overnight warnings arrive.");
    }
    if h.exact && h.exempt {
        ui.weak(
            "Exact alarms and battery exemption are both granted. Samsung's One UI can still put \
             the app to sleep on its own — that switch lives in Settings \u{2192} Battery \u{2192} \
             Background usage limits, and is outside what any app can set for you.",
        );
    }
}
