//! Broadcast scenes (ROADMAP_2 §6.3): save what the output shows — where the map looks, the layers
//! on it, the colour scale, the dressing, the title strap and the output size — as a named scene,
//! and switch to it with Alt+1..9 (in the order saved) or from the list beside the output window's
//! settings. Applying a scene flies the camera there (`app::camera_flight`) rather than cutting.
//!
//! Since scene format 2 (ROADMAP_PARITY M6.2) a scene also keeps the pane's product, tilt, SRV,
//! field layers, thresholds and column product, and whether it is live or one archived instant;
//! an older scene keeps the pane's current product and time for the parts it never stored.
//! Scenes saved since also keep the colour tables and the freehand annotations; the colour
//! scale's side rides in the dressing (`Broadcast::legend_side`).

use super::*;
use crate::broadcast::{Scene, SceneProduct, SceneTime, SceneView3d, SCENE_VERSION};
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
    if scene.version > SCENE_VERSION {
        r.blocking.push(format!(
            "saved by a newer HookEcho (scene format {}, this build reads up to {SCENE_VERSION})",
            scene.version
        ));
    }
    if let Some(p) = &scene.product {
        // A missing product is not swapped for another one: that would put different science
        // on air under the scene's name.
        if let Some(name) = &p.column_product {
            if !settings.udp_products.iter().any(|d| &d.name == name) {
                r.blocking.push(format!(
                    "column product \u{201c}{name}\u{201d} is not defined here"
                ));
            }
        }
        let unknown = p
            .fields_on
            .iter()
            .filter(|s| {
                !crate::render::FieldLayer::DRAW_ORDER
                    .iter()
                    .any(|l| l.slug() == *s)
            })
            .count();
        if unknown > 0 {
            r.notes.push(format!(
                "{unknown} field layer{} this build doesn't have will be skipped",
                if unknown == 1 { "" } else { "s" }
            ));
        }
    }
    if let Some(v3) = &scene.view3d {
        match crate::view::Map3dRepresentation::from_label(&v3.representation) {
            None => r.notes.push(format!(
                "3D mode \u{201c}{}\u{201d} isn't in this build; the 3D map is left as it is",
                v3.representation
            )),
            Some(crate::view::Map3dRepresentation::SmoothProduct) => {
                if let Some(name) = &v3.product {
                    if !settings.udp_products.iter().any(|d| &d.name == name) {
                        r.blocking.push(format!(
                            "3D product \u{201c}{name}\u{201d} is not defined here"
                        ));
                    }
                }
            }
            Some(_) => {}
        }
    }
    if let Some(pals) = &scene.palettes {
        let on_air = scene.product.as_ref().map(|p| p.moment);
        for (key, value) in pals {
            let Some(why) = palette_problem(value, settings) else {
                continue;
            };
            // The scene's own product in another colour table is a different picture under the
            // scene's name; another moment's table is only kept as it is.
            if on_air.is_some_and(|m| palette_key_is(key, m)) {
                r.blocking.push(format!("{key} colour table: {why}"));
            } else {
                r.notes
                    .push(format!("{key} colour table is kept as it is: {why}"));
            }
        }
    }
    if let Some(SceneTime::Fixed { utc }) = scene.time {
        r.notes.push(format!(
            "archive scene: Take loads {} UTC",
            utc.format("%Y-%m-%d %H:%M")
        ));
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

/// Whether `key` in `Settings::palettes` names moment `m`'s table (CC also answers to its
/// pre-rename key, as `Settings::palette_paths` reads it).
fn palette_key_is(key: &str, m: Moment) -> bool {
    key == m.short_name() || (m == Moment::CorrelationCoefficient && key == "RHO")
}

/// Why the colour table a scene names cannot be loaded here, or `None` when it can: a built-in
/// alternate this build lacks, a file that is gone, or one that is not a colour table. A browser's
/// stored file (`Settings::web_files`) carries its own content.
pub(crate) fn palette_problem(value: &str, settings: &crate::settings::Settings) -> Option<String> {
    if let Some(name) = value.strip_prefix(crate::colormap::BUILTIN_PREFIX) {
        return crate::colormap::resolve_builtin(name)
            .is_none()
            .then(|| format!("built-in table \u{201c}{name}\u{201d} is not in this build"));
    }
    let text = match settings.web_files.get(value) {
        Some(text) => text.clone(),
        None => match std::fs::read_to_string(value) {
            Ok(text) => text,
            Err(_) => return Some(format!("no file at {value}")),
        },
    };
    crate::colormap::parse_pal(&text)
        .err()
        .map(|e| format!("{value} is not a colour table ({e})"))
}

/// The colour tables Take leaves set: the scene's, except one that cannot load here, where the
/// table set now stays (readiness has said so, or stopped Take when it is the scene's product).
/// A moment the scene does not name goes back to the built-in table, as it was when saved.
pub(crate) fn scene_palettes(
    scene: &std::collections::BTreeMap<String, String>,
    settings: &crate::settings::Settings,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (key, value) in scene {
        if palette_problem(value, settings).is_none() {
            out.insert(key.clone(), value.clone());
        } else if let Some(now) = settings.palettes.get(key) {
            out.insert(key.clone(), now.clone());
        }
    }
    out
}

/// Annotation strokes as a scene (or a case) stores them.
pub(crate) fn stored_strokes(strokes: &[Stroke2d]) -> Vec<crate::case::CaseStroke> {
    strokes
        .iter()
        .map(|s| crate::case::CaseStroke {
            points: s.points.clone(),
            rgba: s.color.to_array(),
        })
        .collect()
}

/// Stored annotation strokes back on the map.
pub(crate) fn drawn_strokes(stored: &[crate::case::CaseStroke]) -> Vec<Stroke2d> {
    stored
        .iter()
        .map(|s| Stroke2d {
            points: s.points.clone(),
            // `Color32::to_array` wrote it premultiplied; read it back the same way.
            color: egui::Color32::from_rgba_premultiplied(
                s.rgba[0], s.rgba[1], s.rgba[2], s.rgba[3],
            ),
        })
        .collect()
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

/// The product a pane shows, as a scene keeps it. Model layers are left out (see
/// [`SceneProduct::fields_on`]).
pub(crate) fn scene_product(v: &MapView) -> SceneProduct {
    SceneProduct {
        moment: v.moment,
        tilt: v.tilt,
        srv: v.srv,
        fields_on: crate::render::FieldLayer::DRAW_ORDER
            .iter()
            .filter(|l| v.fields_on.contains(l) && !model_context::MODEL_LAYERS.contains(l))
            .map(|l| l.slug().to_string())
            .collect(),
        thresholds: Moment::ALL
            .iter()
            .enumerate()
            .filter(|&(i, _)| v.threshold_enabled[i])
            .filter_map(|(i, m)| v.thresholds[i].map(|t| (*m, t)))
            .collect(),
        column_product: v.column_product.clone(),
    }
}

/// The pane's time as a scene keeps it: live while following the newest frame, else the
/// instant of the frame on screen; `None` when a scrubbed pane has no frame to name.
pub(crate) fn scene_time(v: &MapView) -> Option<SceneTime> {
    if v.timeline.following {
        return Some(SceneTime::Live);
    }
    let utc = v.timeline.current().and_then(|id| id.date_time())?;
    Some(SceneTime::Fixed { utc })
}

/// Put a scene's product on a pane. Its model layers stay as they are; everything else it
/// names replaces what the pane had, thresholds included, so no filter from the previous view
/// stays on air.
pub(crate) fn apply_scene_product(p: &SceneProduct, v: &mut MapView) {
    v.moment = p.moment;
    v.tilt = p.tilt;
    v.srv = p.srv;
    v.thresholds = Default::default();
    v.threshold_enabled = Default::default();
    for (m, t) in &p.thresholds {
        v.thresholds[m.index()] = Some(*t);
        v.threshold_enabled[m.index()] = true;
    }
    v.fields_on = crate::render::FieldLayer::DRAW_ORDER
        .iter()
        .copied()
        .filter(|l| {
            if model_context::MODEL_LAYERS.contains(l) {
                v.fields_on.contains(l)
            } else {
                p.fields_on.iter().any(|s| s == l.slug())
            }
        })
        .collect();
    v.column_product = p.column_product.clone();
}

/// The pane's 3D map as a scene keeps it.
pub(crate) fn scene_view3d(v: &MapView) -> SceneView3d {
    let m = &v.map_3d;
    SceneView3d {
        enabled: m.enabled,
        representation: m.representation.label().to_string(),
        render: m.volume_render,
        vertical_exaggeration: m.vertical_exaggeration,
        opacity: m.opacity,
        quality_steps: m.quality_steps,
        product: m.product.clone(),
    }
}

/// Put a scene's 3D map on a pane. A mode this build lacks leaves the 3D map as it is (the
/// readiness check says so); a region of interest is cleared, since it belonged to a storm of
/// another time.
pub(crate) fn apply_scene_view3d(s: &SceneView3d, v: &mut MapView) {
    let Some(rep) = crate::view::Map3dRepresentation::from_label(&s.representation) else {
        return;
    };
    let m = &mut v.map_3d;
    m.enabled = s.enabled;
    m.representation = rep;
    m.volume_render = s.render;
    m.vertical_exaggeration = s.vertical_exaggeration;
    m.opacity = s.opacity.clamp(0.0, 1.0);
    m.quality_steps = s.quality_steps;
    m.product = s.product.clone();
    m.roi = None;
}

/// Put a scene's time on a pane: back to live, or a seek to the archived instant (the pane's
/// listing for that day loads through the normal path, as an opened case does).
pub(crate) fn apply_scene_time(t: SceneTime, v: &mut MapView) {
    match t {
        SceneTime::Live => v.timeline.go_head(),
        SceneTime::Fixed { utc } => {
            v.timeline.date = utc.date_naive();
            v.timeline.following = false;
            v.timeline.playing = false;
            v.timeline.seek_target = Some(utc);
        }
    }
}

/// A copy of scene `i` right after it, named so it reads as the copy ("Moore (2)", then
/// "Moore (3)").
pub(crate) fn duplicate_scene(scenes: &mut Vec<Scene>, i: usize) {
    let Some(src) = scenes.get(i).cloned() else {
        return;
    };
    let base = match src.name.rsplit_once(" (") {
        Some((b, n)) if n.trim_end_matches(')').parse::<u32>().is_ok() => b.to_string(),
        _ => src.name.clone(),
    };
    let name = (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|n| !scenes.iter().any(|s| &s.name == n))
        .expect("an unused name");
    scenes.insert(i + 1, Scene { name, ..src });
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
            version: SCENE_VERSION,
            product: Some(scene_product(v)),
            time: scene_time(v),
            view3d: Some(scene_view3d(v)),
            palettes: Some(self.settings.palettes.clone()),
            annotations: Some(stored_strokes(&self.strokes)),
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

    /// Put a scene on pane `target`: camera (flown to), product and time, layers, colour scale,
    /// colour tables, annotations, dressing, strap, output size, GIS layers.
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
        if let Some(p) = &scene.product {
            apply_scene_product(p, v);
        }
        // After the product: a 3D mode is checked against the moment the pane now shows.
        if let Some(v3) = &scene.view3d {
            apply_scene_view3d(v3, v);
        }
        if let Some(t) = scene.time {
            apply_scene_time(t, v);
            if self.link_times {
                let time = match t {
                    SceneTime::Live => None,
                    SceneTime::Fixed { utc } => Some(utc),
                };
                self.select_linked_explicit(target, time);
            }
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
        // Applied by the frame's end like any other palette change (`frame_end`).
        if let Some(pals) = &scene.palettes {
            self.settings.palettes = scene_palettes(pals, &self.settings);
        }
        // The scene's drawing replaces what is on the map, so the last scene's arrows do not
        // stay on air over this one.
        if let Some(strokes) = &scene.annotations {
            self.strokes = drawn_strokes(strokes);
        }
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
            let mut duplicate = None;
            let mut swap = None;
            let count = self.settings.scenes.len();
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
                    if ui
                        .small_button("\u{29c9}")
                        .on_hover_text("Duplicate, to make a variation of this scene")
                        .clicked()
                    {
                        duplicate = Some(i);
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("\u{25b2}").small())
                        .on_hover_text("Move up (Alt+1..9 follow the order)")
                        .clicked()
                    {
                        swap = Some((i - 1, i));
                    }
                    if ui
                        .add_enabled(i + 1 < count, egui::Button::new("\u{25bc}").small())
                        .on_hover_text("Move down (Alt+1..9 follow the order)")
                        .clicked()
                    {
                        swap = Some((i, i + 1));
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
                    "{} · {} · {} · zoom {:.1}{}",
                    scene.site.as_deref().unwrap_or("current radar"),
                    scene.product.as_ref().map_or_else(
                        || "current product".to_string(),
                        |p| format!("{} tilt {}", p.moment.short_name(), p.tilt + 1)
                    ),
                    match scene.time {
                        None => "current time".to_string(),
                        Some(SceneTime::Live) => "live".to_string(),
                        Some(SceneTime::Fixed { utc }) =>
                            format!("{} UTC", utc.format("%Y-%m-%d %H:%M")),
                    },
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
            } else if let Some(i) = duplicate {
                duplicate_scene(&mut self.settings.scenes, i);
            } else if let Some((a, b)) = swap {
                self.settings.scenes.swap(a, b);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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

    fn pane() -> MapView {
        MapView::new(
            Some("KTLX".into()),
            crate::render::mercator::Camera::at_lonlat(-97.3, 35.3, 8.0),
        )
    }

    #[test]
    fn a_scene_puts_back_the_product_it_was_saved_with() {
        use crate::render::FieldLayer;
        let mut saved = pane();
        saved.moment = Moment::Velocity;
        saved.tilt = 2;
        saved.srv = true;
        saved.thresholds[Moment::Velocity.index()] = Some(20.0);
        saved.threshold_enabled[Moment::Velocity.index()] = true;
        let radar_layer = FieldLayer::DRAW_ORDER
            .iter()
            .copied()
            .find(|l| !model_context::MODEL_LAYERS.contains(l))
            .unwrap();
        let model_layer = model_context::MODEL_LAYERS[0];
        saved.fields_on = [radar_layer, model_layer].into_iter().collect();
        saved.column_product = Some("VIL".into());
        let product = scene_product(&saved);
        assert!(
            !product.fields_on.contains(&model_layer.slug().to_string()),
            "model layers need a model run a scene does not carry"
        );

        // A pane showing something else entirely, with its own model layer and a filter.
        let mut on_air = pane();
        on_air.moment = Moment::Reflectivity;
        on_air.thresholds[Moment::Reflectivity.index()] = Some(40.0);
        on_air.threshold_enabled[Moment::Reflectivity.index()] = true;
        let other_model = model_context::MODEL_LAYERS[1];
        on_air.fields_on = [other_model].into_iter().collect();
        apply_scene_product(&product, &mut on_air);
        assert_eq!(scene_product(&on_air), product);
        assert_eq!(
            (on_air.moment, on_air.tilt, on_air.srv),
            (Moment::Velocity, 2, true)
        );
        assert!(
            !on_air.threshold_enabled[Moment::Reflectivity.index()],
            "no filter from the previous view stays on air"
        );
        assert!(on_air.fields_on.contains(&radar_layer));
        assert!(
            on_air.fields_on.contains(&other_model) && !on_air.fields_on.contains(&model_layer),
            "the pane's model layers stay as they were"
        );
        assert_eq!(on_air.column_product.as_deref(), Some("VIL"));
    }

    #[test]
    fn a_scene_is_live_or_one_archived_instant() {
        let mut v = pane();
        assert_eq!(scene_time(&v), Some(SceneTime::Live));
        let utc = chrono::Utc.with_ymd_and_hms(2013, 5, 20, 20, 1, 0).unwrap();
        apply_scene_time(SceneTime::Fixed { utc }, &mut v);
        assert!(!v.timeline.following && !v.timeline.playing);
        assert_eq!(v.timeline.seek_target, Some(utc));
        assert_eq!(v.timeline.date, utc.date_naive());
        // Scrubbed with nothing loaded yet there is no frame to name, so nothing is claimed.
        assert_eq!(scene_time(&v), None);
        apply_scene_time(SceneTime::Live, &mut v);
        assert!(v.timeline.following);
        assert_eq!(v.timeline.seek_target, None);
        // Round trip through JSON in both forms.
        for t in [SceneTime::Live, SceneTime::Fixed { utc }] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(
                serde_json::from_str::<SceneTime>(&json).unwrap(),
                t,
                "{json}"
            );
        }
    }

    #[test]
    fn readiness_refuses_a_missing_product_and_a_newer_format() {
        let mut settings = crate::settings::Settings::default();
        let base: Scene =
            serde_json::from_str(r#"{"name":"VIL","lon":-97.0,"lat":35.0,"zoom":8.0}"#).unwrap();
        assert_eq!(base.version, 0, "an older scene");
        assert!(
            base.product.is_none() && base.time.is_none(),
            "keeps the pane's product and time"
        );
        let scene = Scene {
            version: SCENE_VERSION,
            product: Some(SceneProduct {
                moment: Moment::Reflectivity,
                tilt: 0,
                srv: false,
                fields_on: vec!["no-such-field".into()],
                thresholds: Vec::new(),
                column_product: Some("Hail depth".into()),
            }),
            time: Some(SceneTime::Fixed {
                utc: chrono::Utc.with_ymd_and_hms(2013, 5, 20, 20, 1, 0).unwrap(),
            }),
            ..base.clone()
        };
        let r = scene_readiness(&scene, &settings, Ok(0));
        assert_eq!(r.blocking.len(), 1, "{:?}", r.blocking);
        assert!(r.blocking[0].contains("Hail depth"), "{:?}", r.blocking);
        assert!(
            r.notes.iter().any(|n| n.contains("1 field layer")),
            "{:?}",
            r.notes
        );
        assert!(
            r.notes.iter().any(|n| n.contains("2013-05-20 20:01")),
            "{:?}",
            r.notes
        );
        settings.udp_products.push(
            serde_json::from_value(
                serde_json::json!({"name": "Hail depth", "units": "", "expression": "REF"}),
            )
            .unwrap(),
        );
        assert!(scene_readiness(&scene, &settings, Ok(0))
            .blocking
            .is_empty());
        let newer = Scene {
            version: SCENE_VERSION + 1,
            ..scene
        };
        let r = scene_readiness(&newer, &settings, Ok(0));
        assert!(
            r.blocking.iter().any(|b| b.contains("newer HookEcho")),
            "{:?}",
            r.blocking
        );
        // A saved scene round-trips whole.
        let back: Scene = serde_json::from_str(&serde_json::to_string(&newer).unwrap()).unwrap();
        assert_eq!(back, newer);
    }

    #[test]
    fn a_scene_puts_back_its_3d_map_and_clears_a_storm_region() {
        use crate::view::Map3dRepresentation as R;
        let mut saved = pane();
        saved.map_3d.enabled = true;
        saved.map_3d.representation = R::SmoothVolume;
        saved.map_3d.volume_render = crate::render3d::VolumeRender::TranslucentLit;
        saved.map_3d.vertical_exaggeration = 3.0;
        saved.map_3d.opacity = 0.5;
        saved.map_3d.quality_steps = 96;
        let v3 = scene_view3d(&saved);
        assert_eq!(v3.representation, "Smooth reflectivity");
        let mut on_air = pane();
        on_air.map_3d.roi = Some(crate::view::VolumeRoi {
            center: [-97.0, 35.0],
            half_km: 20.0,
            storm: Some("Q4".into()),
            follow: true,
            lost: false,
        });
        apply_scene_view3d(&v3, &mut on_air);
        assert_eq!(scene_view3d(&on_air), v3);
        assert!(
            on_air.map_3d.roi.is_none(),
            "a region belonged to another time's storm"
        );
        // A mode this build lacks leaves the 3D map alone, and readiness says so.
        let unknown = SceneView3d {
            representation: "Hologram".into(),
            ..v3.clone()
        };
        let before = on_air.map_3d.clone();
        apply_scene_view3d(&unknown, &mut on_air);
        assert_eq!(
            on_air.map_3d.vertical_exaggeration,
            before.vertical_exaggeration
        );
        let base: Scene =
            serde_json::from_str(r#"{"name":"3D","lon":-97.0,"lat":35.0,"zoom":8.0}"#).unwrap();
        assert!(base.view3d.is_none(), "an older scene leaves 3D as it is");
        let settings = crate::settings::Settings::default();
        let r = scene_readiness(
            &Scene {
                view3d: Some(unknown),
                ..base.clone()
            },
            &settings,
            Ok(0),
        );
        assert!(
            r.notes.iter().any(|n| n.contains("Hologram")),
            "{:?}",
            r.notes
        );
        // A user-product volume whose product is gone is not substituted.
        let product = SceneView3d {
            representation: "User product".into(),
            product: Some("Hail depth".into()),
            ..v3
        };
        let r = scene_readiness(
            &Scene {
                view3d: Some(product),
                ..base
            },
            &settings,
            Ok(0),
        );
        assert!(
            r.blocking.iter().any(|b| b.contains("Hail depth")),
            "{:?}",
            r.blocking
        );
    }

    #[test]
    fn a_scene_keeps_its_colour_tables_and_never_airs_a_missing_one() {
        let hc = "builtin:High contrast (reflectivity)";
        let dir = std::env::temp_dir().join(format!("hookecho-scene-pal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let not_a_table = dir.join("notes.pal");
        std::fs::write(&not_a_table, "hello").unwrap();
        let gone = dir.join("gone.pal");
        let mut settings = crate::settings::Settings::default();
        settings
            .palettes
            .insert("VEL".into(), "builtin:something-set-now".into());
        settings.palettes.insert("ZDR".into(), "zdr-now.pal".into());
        settings.web_files.insert(
            "cc.pal".into(),
            crate::colormap::to_pal_string(crate::colormap::default_table(
                Moment::CorrelationCoefficient,
            )),
        );
        let pals: std::collections::BTreeMap<String, String> = [
            ("REF".to_string(), hc.to_string()),
            ("VEL".to_string(), gone.display().to_string()),
            ("ZDR".to_string(), not_a_table.display().to_string()),
            ("CC".to_string(), "cc.pal".to_string()),
        ]
        .into();
        assert!(palette_problem(hc, &settings).is_none());
        assert!(
            palette_problem("cc.pal", &settings).is_none(),
            "a browser's stored file"
        );
        assert!(palette_problem("builtin:Nope", &settings)
            .unwrap()
            .contains("Nope"));
        assert!(palette_problem(&gone.display().to_string(), &settings)
            .unwrap()
            .contains("no file"));
        assert!(
            palette_problem(&not_a_table.display().to_string(), &settings)
                .unwrap()
                .contains("not a colour table")
        );
        let base: Scene =
            serde_json::from_str(r#"{"name":"Pal","lon":-97.0,"lat":35.0,"zoom":8.0}"#).unwrap();
        assert!(
            base.palettes.is_none() && base.annotations.is_none(),
            "an older scene"
        );
        let product = |moment| SceneProduct {
            moment,
            tilt: 0,
            srv: false,
            fields_on: Vec::new(),
            thresholds: Vec::new(),
            column_product: None,
        };
        // On air in reflectivity: the broken velocity and ZDR tables are notes.
        let refl = Scene {
            palettes: Some(pals.clone()),
            product: Some(product(Moment::Reflectivity)),
            ..base.clone()
        };
        let r = scene_readiness(&refl, &settings, Ok(0));
        assert!(r.blocking.is_empty(), "{:?}", r.blocking);
        assert_eq!(r.notes.len(), 2, "{:?}", r.notes);
        assert!(
            r.notes.iter().all(|n| n.contains("kept as it is")),
            "{:?}",
            r.notes
        );
        // On air in velocity, its own table missing stops Take.
        let vel = Scene {
            product: Some(product(Moment::Velocity)),
            ..refl.clone()
        };
        let r = scene_readiness(&vel, &settings, Ok(0));
        assert_eq!(r.blocking.len(), 1, "{:?}", r.blocking);
        assert!(
            r.blocking[0].starts_with("VEL colour table: no file"),
            "{:?}",
            r.blocking
        );
        // What Take sets: the scene's tables, the current ones where the scene's cannot load,
        // and the built-in for a moment the scene does not name.
        settings.palettes.insert("SW".into(), "sw-now.pal".into());
        let set = scene_palettes(&pals, &settings);
        assert_eq!(set.get("REF").map(String::as_str), Some(hc));
        assert_eq!(
            set.get("VEL").map(String::as_str),
            Some("builtin:something-set-now")
        );
        assert_eq!(set.get("ZDR").map(String::as_str), Some("zdr-now.pal"));
        assert_eq!(set.get("CC").map(String::as_str), Some("cc.pal"));
        assert!(
            !set.contains_key("SW"),
            "not in the scene: back to the built-in table"
        );
        let back: Scene = serde_json::from_str(&serde_json::to_string(&refl).unwrap()).unwrap();
        assert_eq!(back, refl);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scene_keeps_its_annotations_in_their_colours() {
        let drawn = vec![
            Stroke2d {
                points: vec![[-97.5, 35.3], [-97.4, 35.35]],
                color: DRAW_COLORS[0],
            },
            Stroke2d {
                points: vec![[-97.0, 35.0]],
                color: egui::Color32::from_rgba_unmultiplied(90, 220, 255, 128),
            },
        ];
        let stored = stored_strokes(&drawn);
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "name": "Arrows", "lon": -97.0, "lat": 35.0, "zoom": 8.0,
            "annotations": serde_json::to_value(&stored).unwrap()
        }))
        .unwrap();
        assert_eq!(drawn_strokes(scene.annotations.as_deref().unwrap()), drawn);
        // An empty drawing is kept as empty: Take clears the last scene's arrows.
        let none: Scene = serde_json::from_value(serde_json::json!({
            "name": "Clean", "lon": -97.0, "lat": 35.0, "zoom": 8.0, "annotations": []
        }))
        .unwrap();
        assert_eq!(none.annotations, Some(Vec::new()));
    }

    #[test]
    fn the_scale_side_rides_in_the_dressing_and_older_styles_keep_it_right() {
        use crate::broadcast::{Broadcast, LegendSide};
        let old: Broadcast = serde_json::from_str(r#"{"legend":true,"clock":false}"#).unwrap();
        assert_eq!(old.legend_side, LegendSide::Right);
        let left = Broadcast {
            legend_side: LegendSide::Left,
            ..old
        };
        let json = serde_json::to_string(&left).unwrap();
        assert!(json.contains(r#""legend_side":"left""#), "{json}");
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "name": "L", "lon": -97.0, "lat": 35.0, "zoom": 8.0,
            "style": serde_json::from_str::<serde_json::Value>(&json).unwrap()
        }))
        .unwrap();
        assert_eq!(scene.style.legend_side, LegendSide::Left);
        // The scale's strip on the left: its bar sits in from the map's left edge.
        let map = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0));
        assert_eq!(
            crate::ui::legend::vertical_rect(map, LegendSide::Right),
            map
        );
        let strip = crate::ui::legend::vertical_rect(map, LegendSide::Left);
        assert_eq!(strip.left(), map.left());
        assert!(strip.right() < map.left() + crate::ui::legend::VERTICAL_CLEAR);
        assert_eq!(strip.height(), map.height());
    }

    #[test]
    fn a_duplicate_lands_after_its_original_with_a_free_name() {
        let scene = |name: &str| -> Scene {
            serde_json::from_value(serde_json::json!({
                "name": name, "lon": -97.0, "lat": 35.0, "zoom": 8.0
            }))
            .unwrap()
        };
        let mut scenes = vec![scene("Moore"), scene("Overview")];
        duplicate_scene(&mut scenes, 0);
        duplicate_scene(&mut scenes, 0);
        let names: Vec<&str> = scenes.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Moore", "Moore (3)", "Moore (2)", "Overview"]);
        // A copy of a copy counts on from the base name.
        duplicate_scene(&mut scenes, 1);
        assert_eq!(scenes[2].name, "Moore (4)");
        duplicate_scene(&mut scenes, 99);
        assert_eq!(scenes.len(), 5, "out of range does nothing");
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
