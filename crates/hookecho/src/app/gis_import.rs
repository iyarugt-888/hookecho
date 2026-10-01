//! Importing GIS files (KML, GeoJSON, shapefiles) into the imported layer.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Route a picked file to whatever asked for it.
    pub(crate) fn apply_import(&mut self, import: crate::dialog::Import) {
        use crate::dialog::ImportKind as K;
        match import.kind {
            // Routed to `open_case` before this, which needs the egui context.
            K::Case => {}
            K::SettingsBundle => self.apply_settings_bundle(&import),
            K::Palette if import.tag == crate::ui::palette_editor::EDITOR_TAG => {
                match import.text() {
                    Ok(text) => self.palette_editor.pending_import = Some(text),
                    Err(e) => self.toast(ToastKind::Error, format!("Palette import failed: {e}")),
                }
            }
            K::Palette => {
                // A platform that handed over content rather than a path (the browser) has no
                // path worth storing — the content goes into the settings and the override names
                // it, which is what `palette_paths` resolves back to a table.
                let value = match &import.bytes {
                    None => import.path.to_string_lossy().into_owned(),
                    Some(_) => match import.text() {
                        Ok(text) => {
                            let name = import.name();
                            self.settings.web_files.insert(name.clone(), text);
                            name
                        }
                        Err(e) => {
                            self.toast(ToastKind::Error, format!("Palette import failed: {e}"));
                            return;
                        }
                    },
                };
                // Setting the override triggers the next-frame dirty-diff palette reload.
                self.settings.palettes.insert(import.tag, value);
            }
            K::ChaseGpx => match import.text() {
                Ok(xml) => {
                    let track = crate::chaselog::from_gpx(&xml);
                    if track.points.len() < 2 {
                        self.toast(ToastKind::Error, "No track points in that file".to_string());
                    } else {
                        let n = track.points.len();
                        self.chase_replay.load(track, import.name());
                        self.toast(ToastKind::Info, format!("Loaded {n} track points"));
                    }
                }
                Err(e) => self.toast(ToastKind::Error, format!("GPX import failed: {e}")),
            },
            K::MarkerIcon => {
                let idx = import.tag.parse::<usize>().ok();
                match (
                    idx.and_then(|i| self.settings.markers.get_mut(i)),
                    crate::ui::marker_window::store_icon(&import.path),
                ) {
                    (Some(m), Some(name)) => m.icon = Some(name),
                    _ => log::warn!("marker icon import went nowhere (marker {})", import.tag),
                }
            }
            K::AlertSound => {
                let file = import.path.to_string_lossy().into_owned();
                let sound = crate::settings::AlertSound::Custom(file);
                match import.tag.as_str() {
                    "New scan" => self.settings.scan_sound = sound,
                    "Warning" => self.settings.warn_sound = sound,
                    "Emergency" => self.settings.emergency_sound = sound,
                    "TDS" => self.settings.tds_sound = sound,
                    "Rotation" => self.settings.rotation_sound = sound,
                    "Lightning" => self.settings.lightning_sound = sound,
                    other => log::warn!("no alert sound row named '{other}'"),
                }
            }
            K::GisFile => match crate::gis_import::load_import(&import) {
                Ok(loaded) => {
                    // Remember it the same two ways an imported `.pal` is remembered: a path
                    // where there is a filesystem, the content itself in a browser, which has
                    // no path that would survive a reload. A boundary file someone works with
                    // daily should not need re-picking on every launch. A browser's shapefile or
                    // KMZ is binary and `web_files` holds text, so it is kept as the GeoJSON it
                    // reads back as — lossless, since export and import share one type.
                    let remembered = match &import.bytes {
                        None => Ok(import.path.to_string_lossy().into_owned()),
                        Some(_) if crate::gis_import::is_binary(&import.name()) => {
                            let stem = import.path.file_stem().map_or_else(
                                || "import".to_string(),
                                |s| s.to_string_lossy().into_owned(),
                            );
                            let name = format!("{stem}.geojson");
                            self.settings
                                .web_files
                                .insert(name.clone(), wxdata::gis::to_geojson(&loaded.features));
                            Ok(name)
                        }
                        Some(_) => import.text().map(|text| {
                            let name = import.name();
                            self.settings.web_files.insert(name.clone(), text);
                            name
                        }),
                    };
                    let (shapes, marks) = crate::gis_import::to_renderable(loaded.features);
                    let n = shapes.len() + marks.len();
                    self.imported_gis = shapes;
                    self.imported_marks = marks;
                    self.imported_colors = None;
                    self.imported_time = None;
                    self.show_imported_gis = true;
                    self.rebuild_overlays();
                    self.settings.imported_gis = remembered.ok();
                    // Framing the import is the difference between "nothing happened" and
                    // "there it is" for a file covering somewhere the map isn't looking.
                    self.zoom_to_imported_gis();
                    let mut message = format!("Imported {n} shapes from {}", import.name());
                    if let Some(note) = &loaded.note {
                        message.push_str(&format!(" — {note}"));
                    }
                    self.toast(
                        if n == 0 {
                            ToastKind::Error
                        } else {
                            ToastKind::Info
                        },
                        message,
                    );
                }
                Err(e) => self.toast(ToastKind::Error, format!("GIS import failed: {e}")),
            },
        }
    }
}
