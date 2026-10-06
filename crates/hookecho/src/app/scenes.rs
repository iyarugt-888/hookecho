//! Broadcast scenes (ROADMAP_2 §6.3): save what the output shows — where the map looks, the layers
//! on it, the colour scale, the dressing, the title strap and the output size — as a named scene,
//! and switch to it with Alt+1..9 (in the order saved) or from the list beside the output window's
//! settings. Applying a scene flies the camera there (`app::camera_flight`) rather than cutting.

use super::*;
use crate::broadcast::Scene;
use output_window::{program_pane, OutputSize};

/// Whether a scene going into pane `target` sets program's held camera rather than the pane's:
/// the output is open, holding its view, and `target` is its pane.
pub(crate) fn takes_held_camera(
    output: &output_window::OutputWindow,
    active: usize,
    panes: usize,
    target: usize,
) -> bool {
    output.open && output.hold && program_pane(output.source, active, panes) == Ok(target)
}

/// What a scene would do on Take (ROADMAP_PARITY M6.1): problems that stop it, so program keeps
/// what it shows, and notes on what it will skip.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Readiness {
    pub blocking: Vec<String>,
    pub notes: Vec<String>,
}

/// Check a scene against this build and session before it goes on air: where it would go, layer
/// names this build lacks, imported GIS layers it shows that are gone, an output size it names
/// that this build does not have.
pub(crate) fn scene_readiness(
    scene: &Scene,
    settings: &crate::settings::Settings,
    target: Result<usize, String>,
) -> Readiness {
    let mut r = Readiness::default();
    if let Err(why) = target {
        r.blocking.push(format!("program has no pane: {why}"));
    }
    let unknown = scene
        .overlays_on
        .iter()
        .filter(|s| OverlayToggle::from_slug(s).is_none())
        .count();
    if unknown > 0 {
        r.notes.push(format!(
            "{unknown} layer{} this build doesn't have will be skipped",
            if unknown == 1 { "" } else { "s" }
        ));
    }
    if let Some(gis) = &scene.gis {
        let missing = settings.clone().apply_gis_snapshot(gis);
        if !missing.is_empty() {
            r.notes.push(format!(
                "GIS layers no longer imported: {}",
                missing.join(", ")
            ));
        }
    }
    if !scene.size.is_empty() && OutputSize::from_label(&scene.size).is_none() {
        r.notes.push(format!(
            "output size \u{201c}{}\u{201d} is kept as it is",
            scene.size
        ));
    }
    r
}

impl OutputSize {
    pub(crate) fn from_label(label: &str) -> Option<OutputSize> {
        [
            OutputSize::Hd1080,
            OutputSize::Qhd1440,
            OutputSize::Uhd2160,
            OutputSize::Free,
        ]
        .into_iter()
        .find(|s| s.label() == label)
    }
}

/// Which scene an Alt+1..9 press this frame asks for (0-based), consuming the key.
fn scene_key(ctx: &egui::Context) -> Option<usize> {
    use egui::Key::*;
    let keys = [Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9];
    ctx.input_mut(|i| {
        keys.iter()
            .position(|k| i.consume_key(egui::Modifiers::ALT, *k))
    })
}

impl HookEchoApp {
    /// The active pane and the broadcast settings as a scene called `name`.
    pub(crate) fn capture_scene(&mut self, name: String) -> Scene {
        let mut overlays_on = Vec::new();
        for t in OverlayToggle::ALL {
            if !t.session_only() && *self.overlay_flag(t) {
                overlays_on.push(t.slug());
            }
        }
        let v = &self.views[self.active];
        let (lon, lat) =
            crate::render::mercator::world_to_lonlat(v.camera.center.0, v.camera.center.1);
        Scene {
            name,
            site: v.site.clone(),
            lon,
            lat,
            zoom: v.camera.zoom,
            pitch: v.camera.pitch,
            bearing: v.camera.bearing,
            overlays_on,
            legend: v.show_legend,
            style: self.settings.broadcast.clone(),
            strap: self.output.strap.clone(),
            size: self.output.size.label().to_string(),
            gis: self.settings.gis_snapshot(self.show_imported_gis),
        }
    }

    /// The pane a scene goes into: program's while the output window is open, else the active
    /// pane (streaming mode on the main map).
    pub(crate) fn scene_target(&self) -> Result<usize, String> {
        if self.output.open {
            program_pane(self.output.source, self.active, self.views.len())
        } else {
            Ok(self.active.min(self.views.len() - 1))
        }
    }

    /// Take a scene: checked first, then applied whole, or not at all — a scene that cannot go on
    /// leaves program as it was and says why. The one path for the Take button and Alt+1..9.
    pub(crate) fn take_scene(&mut self, scene: &Scene) {
        let ready = scene_readiness(scene, &self.settings, self.scene_target());
        if !ready.blocking.is_empty() {
            self.toast(
                ToastKind::Error,
                format!(
                    "Scene \u{201c}{}\u{201d} not taken: {}",
                    scene.name,
                    ready.blocking.join("; ")
                ),
            );
            return;
        }
        if let Ok(target) = self.scene_target() {
            self.apply_scene_to(target, scene);
        }
        self.output.cued = None;
    }

    /// Put a scene on pane `target`: camera (flown to), layers, colour scale, dressing, strap,
    /// output size, GIS layers.
    fn apply_scene_to(&mut self, target: usize, scene: &Scene) {
        let camera = crate::render::mercator::Camera {
            center: crate::render::mercator::lonlat_to_world(scene.lon, scene.lat),
            zoom: scene.zoom,
            pitch: scene.pitch,
            bearing: scene.bearing,
        };
        // A held program view takes the scene's camera itself, leaving the operator's view of
        // the pane where it is.
        let holds = takes_held_camera(&self.output, self.active, self.views.len(), target);
        let v = &mut self.views[target];
        if holds {
            self.output.held = Some(camera);
        } else {
            v.camera = camera;
            v.camera_placed = true;
        }
        v.show_legend = scene.legend;
        if let Some(site) = &scene.site {
            v.site = Some(site.clone());
        }
        for t in OverlayToggle::ALL {
            if t.session_only() {
                continue;
            }
            *self.overlay_flag(t) = scene
                .overlays_on
                .iter()
                .any(|s| OverlayToggle::from_slug(s) == Some(t));
        }
        self.settings.broadcast = scene.style.clone();
        self.output.strap = scene.strap.clone();
        if let Some(size) = OutputSize::from_label(&scene.size) {
            self.output.size = size;
        }
        if let Some(gis) = &scene.gis {
            self.apply_gis_snapshot(gis, &format!("Scene \u{201c}{}\u{201d}", scene.name));
        }
        self.rebuild_overlays();
    }

    /// Alt+1..9 applies the scene saved in that place, unless a text field has the keyboard.
    pub(crate) fn scene_hotkeys(&mut self, ctx: &egui::Context) {
        if self.settings.scenes.is_empty() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if let Some(scene) = scene_key(ctx).and_then(|n| self.settings.scenes.get(n).cloned()) {
            self.take_scene(&scene);
        }
    }

    /// The scene list, beside the output window's settings.
    pub(crate) fn scene_rows(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Scenes", |ui| {
            if self.settings.scenes.is_empty() {
                ui.weak("Save the view, layers and dressing as a scene; Alt+1..9 switch to them.");
            }
            let mut go = None;
            let mut remove = None;
            for (i, s) in self.settings.scenes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let key = if i < 9 {
                        format!("Alt+{}", i + 1)
                    } else {
                        String::new()
                    };
                    ui.label(egui::RichText::new(key).monospace().weak());
                    let cued = self.output.cued.as_ref().is_some_and(|c| c == s);
                    if ui
                        .selectable_label(cued, &s.name)
                        .on_hover_text("Cue this scene in preview; Take puts it on")
                        .clicked()
                    {
                        go = Some(i);
                    }
                    if ui.small_button("\u{d7}").on_hover_text("Delete").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if ui.button("Save the current view as a scene").clicked() {
                let n = self.settings.scenes.len() + 1;
                let scene = self.capture_scene(format!("Scene {n}"));
                self.settings.scenes.push(scene);
            }
            if let Some(i) = go {
                self.output.cued = Some(self.settings.scenes[i].clone());
            }
            // Preview: the cued scene, what Take would do with it, and Take.
            if let Some(scene) = self.output.cued.clone() {
                let ready = scene_readiness(&scene, &self.settings, self.scene_target());
                ui.separator();
                ui.label(egui::RichText::new(format!("Preview: {}", scene.name)).strong());
                ui.weak(format!(
                    "{} · zoom {:.1}{}",
                    scene.site.as_deref().unwrap_or("current radar"),
                    scene.zoom,
                    if scene.strap.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" · \u{201c}{}\u{201d}", scene.strap.trim())
                    }
                ));
                match self.scene_target() {
                    Ok(t) => ui.weak(format!("Take puts it on pane {}", t + 1)),
                    Err(_) => ui.weak(""),
                };
                for b in &ready.blocking {
                    ui.colored_label(egui::Color32::from_rgb(230, 100, 70), b);
                }
                for n in &ready.notes {
                    ui.weak(n);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(ready.blocking.is_empty(), egui::Button::new("Take"))
                        .on_hover_text("Put the whole scene on at once")
                        .clicked()
                    {
                        self.take_scene(&scene);
                    }
                    if ui.button("Clear preview").clicked() {
                        self.output.cued = None;
                    }
                });
            }
            if let Some(i) = remove {
                self.settings.scenes.remove(i);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_program_takes_the_scene_camera_and_the_operator_keeps_theirs() {
        use output_window::{OutputWindow, ProgramSource};
        let mut o = OutputWindow::default();
        o.open = true;
        o.hold = true;
        o.source = ProgramSource::Pane(1);
        assert!(takes_held_camera(&o, 0, 2, 1), "program's pane, held");
        assert!(
            !takes_held_camera(&o, 0, 2, 0),
            "another pane takes it as before"
        );
        o.hold = false;
        assert!(
            !takes_held_camera(&o, 0, 2, 1),
            "not held: the pane's camera moves"
        );
        o.hold = true;
        o.open = false;
        assert!(
            !takes_held_camera(&o, 0, 2, 1),
            "no output window, nothing held"
        );
    }

    #[test]
    fn a_scene_is_checked_before_it_goes_on() {
        let scene: Scene = serde_json::from_str(
            r#"{"name":"Tor","lon":-97.0,"lat":35.0,"zoom":8.0,
                "overlays_on":["Couplets","no-such-layer"],"size":"9 × 9"}"#,
        )
        .unwrap();
        let mut settings = crate::settings::Settings::default();
        let ok = scene_readiness(&scene, &settings, Ok(0));
        assert!(ok.blocking.is_empty());
        assert_eq!(ok.notes.len(), 2, "{:?}", ok.notes);
        assert!(ok.notes[0].contains("1 layer"), "{:?}", ok.notes);
        // A program pane that is gone stops Take.
        let gone = scene_readiness(&scene, &settings, Err("pane 2 is not open".into()));
        assert_eq!(gone.blocking, ["program has no pane: pane 2 is not open"]);
        // A GIS layer it shows that has been removed is said, and the check changes nothing.
        let id = settings.add_gis_layer("sirens.geojson".into());
        let with_gis = Scene {
            gis: settings.gis_snapshot(true),
            ..scene
        };
        settings.remove_gis_layer(id);
        let before = settings.clone();
        let r = scene_readiness(&with_gis, &settings, Ok(0));
        assert!(
            r.notes.iter().any(|n| n.contains("sirens.geojson")),
            "{:?}",
            r.notes
        );
        assert_eq!(settings, before);
    }

    #[test]
    fn alt_and_a_digit_picks_that_scene_and_a_bare_digit_does_not() {
        let press = |key, modifiers| egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut got = None;
        let _ = ctx.run_ui(press(egui::Key::Num3, egui::Modifiers::ALT), |ui| {
            got = scene_key(ui.ctx());
        });
        assert_eq!(got, Some(2));
        let _ = ctx.run_ui(press(egui::Key::Num3, egui::Modifiers::NONE), |ui| {
            got = scene_key(ui.ctx());
        });
        assert_eq!(got, None, "a bare 3 is a product key, not a scene");
    }

    #[test]
    fn a_scene_keeps_its_output_size_by_label_and_old_files_still_load() {
        for s in [
            OutputSize::Hd1080,
            OutputSize::Qhd1440,
            OutputSize::Uhd2160,
            OutputSize::Free,
        ] {
            assert_eq!(OutputSize::from_label(s.label()), Some(s));
        }
        assert_eq!(
            OutputSize::from_label("8K"),
            None,
            "unknown keeps the current size"
        );
        let scene: Scene =
            serde_json::from_str(r#"{"name":"Moore","lon":-97.49,"lat":35.33,"zoom":9.0}"#)
                .expect("a minimal scene loads");
        assert_eq!(scene.name, "Moore");
        assert!(scene.overlays_on.is_empty() && scene.strap.is_empty());
    }
}
