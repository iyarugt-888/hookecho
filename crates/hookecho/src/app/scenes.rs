//! Broadcast scenes (ROADMAP_2 §6.3): save what the output shows — where the map looks, the layers
//! on it, the colour scale, the dressing, the title strap and the output size — as a named scene,
//! and switch to it with Alt+1..9 (in the order saved) or from the list beside the output window's
//! settings. Applying a scene flies the camera there (`app::camera_flight`) rather than cutting.

use super::*;
use crate::broadcast::Scene;
use output_window::OutputSize;

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

    /// Put a scene on: camera (flown to), layers, colour scale, dressing, strap, output size.
    pub(crate) fn apply_scene(&mut self, scene: &Scene) {
        let active = self.active;
        let v = &mut self.views[active];
        v.camera = crate::render::mercator::Camera {
            center: crate::render::mercator::lonlat_to_world(scene.lon, scene.lat),
            zoom: scene.zoom,
            pitch: scene.pitch,
            bearing: scene.bearing,
        };
        v.camera_placed = true;
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
            self.apply_scene(&scene);
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
                    if ui
                        .button(&s.name)
                        .on_hover_text("Switch to this scene")
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
                let scene = self.settings.scenes[i].clone();
                self.apply_scene(&scene);
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
