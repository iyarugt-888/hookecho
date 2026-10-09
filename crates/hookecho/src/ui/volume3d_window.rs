//! 3D reflectivity window: an orbitable maximum-intensity raymarch of the volume, drawn by an
//! `egui_wgpu` paint callback. Drag to orbit, scroll to zoom, and cut the volume down to the part
//! you care about with a reflectivity floor and three axis slabs.

use crate::render3d::{orbit_uniform, threshold_index, View3d, Volume3dCallback, Volume3dUpload};
use wxdata::level2::temporal::{TemporalCoverage, TemporalPolicy};

/// Frame labels describe the accepted grid, never the currently requested scan.
#[derive(Clone, Debug)]
pub struct VolumeFrameLabel {
    pub site: Option<String>,
    pub volume: String,
    pub revision: u64,
    pub acquisition: Option<crate::live_scan::AcquisitionSnapshot>,
}

/// Everything the window keeps between frames: the orbit camera, the dBZ floor, the slab, and the
/// layer-by-layer tilt selection.
#[derive(Clone, Debug)]
pub struct Volume3dState {
    pub az: f32,
    pub el: f32,
    pub dist: f32,
    /// Reflectivity floor in dBZ. Anything weaker is treated as empty.
    pub threshold_dbz: f32,
    /// Slab bounds as fractions of the box, `[x0, x1, y0, y1, z0, z1]`.
    pub clip: [f32; 6],
    /// An additional vertical clip plane at any bearing (Phase H4), independent of the
    /// axis-aligned slab above. `None` disables it.
    pub plane: Option<crate::render3d::VerticalPlane>,
    /// Whether to draw a horizontal reference plane at the CAPPI window's shared altitude
    /// (Phase H4's last open item). Off by default, same as `plane`.
    pub cappi_marker: bool,
    /// Raymarch samples per pixel. The cost of the window is almost entirely this number, so it
    /// is the one knob worth exposing on a phone or an integrated GPU.
    pub steps: u32,
    /// MIP or translucent compositing (ROADMAP_PARITY M3.5).
    pub render: crate::render3d::VolumeRender,
    /// The accepted grid's half-width, km: the kilometre scale translucent opacity is per.
    pub half_km: f32,
    /// The volume's tilts, for the layer-by-layer list (set by the app with each build).
    pub layers: Vec<wxdata::level2::ObservedLayer>,
    /// Tilts pulled out, by elevation: when any are, the window shows just those tilts' beams
    /// (`volume3d::build_shells`) instead of the interpolated volume.
    pub selected_elevs: Vec<f32>,
    pub frame: Option<(VolumeFrameLabel, TemporalCoverage)>,
    /// Only draw a GPU grid whose accepted source and controls match the current request.
    pub current: bool,
    pub building: bool,
    pub error: Option<String>,
    pub retry: bool,
}

/// The three quality rungs, coarsest first. 256 is what the window shipped with.
const STEP_PRESETS: [(&str, u32); 3] = [("Low", 96), ("Medium", 160), ("High", 256)];

impl Default for Volume3dState {
    fn default() -> Self {
        Self {
            az: 30.0,
            el: 25.0,
            dist: 3.0,
            // The volume's own floor: nothing hidden until the user asks.
            threshold_dbz: f32::NEG_INFINITY,
            clip: [0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            plane: None,
            cappi_marker: false,
            // ponytail: a phone is the one place the full march reliably misses frame budget, so
            // pick by platform rather than benchmarking the GPU.
            steps: if cfg!(target_os = "android") { 96 } else { 256 },
            render: crate::render3d::VolumeRender::Mip,
            half_km: 50.0,
            layers: Vec::new(),
            selected_elevs: Vec::new(),
            frame: None,
            current: false,
            building: false,
            error: None,
            retry: false,
        }
    }
}

/// The layer-by-layer tilt list, shared by the 3D map's Observed mode and the 3D Reflectivity
/// window: every tilt (highest first, as the stack reads), click to pull one out, click more to
/// compare, with each selected tilt's coverage, strongest value and scan time below. At most
/// [`crate::view::MAX_HIGHLIGHTED_LAYERS`] at once.
pub(crate) fn layers_section(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + Copy + std::fmt::Debug,
    layers: &[wxdata::level2::ObservedLayer],
    selected: &mut Vec<f32>,
    units: &str,
    hint: &str,
) {
    use crate::view::MAX_HIGHLIGHTED_LAYERS as CAP;
    let same = |a: f32, b: f32| (a - b).abs() < 0.05;
    egui::CollapsingHeader::new(format!("Layers ({})", layers.len()))
        .id_salt(("layers_section", id_salt))
        .default_open(false)
        .show(ui, |ui| {
            ui.weak(hint);
            // A fixed cap rather than a scroll area sized to every tilt, so a 19-tilt VCP with
            // MESO-SAILS cuts still fits a floating panel without pushing the controls below it
            // off-screen.
            egui::ScrollArea::vertical()
                .id_salt(("layers_section_scroll", id_salt))
                .max_height(180.0)
                .show(ui, |ui| {
                    for layer in layers.iter().rev() {
                        let on = selected.iter().any(|&e| same(e, layer.elevation_deg));
                        let at_cap = !on && selected.len() >= CAP;
                        let mut resp = ui.selectable_label(
                            on,
                            format!(
                                "{:.1}°  ·  {} radials",
                                layer.elevation_deg, layer.radial_count
                            ),
                        );
                        if at_cap {
                            resp = resp.on_hover_text(format!(
                                "Up to {CAP} tilts can be pulled out at once — deselect one first"
                            ));
                        }
                        if resp.clicked() {
                            if let Some(i) =
                                selected.iter().position(|&e| same(e, layer.elevation_deg))
                            {
                                selected.remove(i);
                            } else if !at_cap {
                                selected.push(layer.elevation_deg);
                            }
                        }
                    }
                });
            if selected.is_empty() {
                return;
            }
            ui.separator();
            let mut chosen = selected.clone();
            chosen.sort_by(|a, b| b.total_cmp(a));
            for sel in chosen {
                let Some(layer) = layers.iter().find(|l| same(l.elevation_deg, sel)) else {
                    continue;
                };
                let total = (layer.radial_count * layer.gate_count).max(1) as f32;
                let coverage_pct = 100.0 * layer.coverage_gates as f32 / total;
                ui.label(format!(
                    "{:.1}°  ·  {} radials × {} gates · {coverage_pct:.0}% coverage",
                    layer.elevation_deg, layer.radial_count, layer.gate_count
                ));
                if let Some(v) = layer.max_value {
                    ui.label(format!("   strongest reading: {v:.1} {units}"));
                }
                match (layer.scan_start, layer.scan_end) {
                    (Some(a), Some(b)) if a != b => {
                        ui.label(format!(
                            "   scanned {} – {} UTC",
                            a.format("%H:%M:%S"),
                            b.format("%H:%M:%S")
                        ));
                    }
                    (Some(a), _) => {
                        ui.label(format!("   scanned {} UTC", a.format("%H:%M:%S")));
                    }
                    _ => {
                        ui.weak("   no per-radial timestamps");
                    }
                }
            }
            if ui.small_button("Clear selection").clicked() {
                selected.clear();
            }
        });
}

/// One `min..max` pair of sliders for an axis of the slab.
fn axis_slice(ui: &mut egui::Ui, label: &str, lo: &mut f32, hi: &mut f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        crate::theme::slider(ui, egui::Slider::new(lo, 0.0..=1.0).show_value(false));
        crate::theme::slider(ui, egui::Slider::new(hi, 0.0..=1.0).show_value(false));
    });
    // Keep the pair ordered so an inverted drag empties the view instead of inverting the slab.
    if *lo > *hi {
        std::mem::swap(lo, hi);
    }
}

/// A transfer function's stops, in a moment's own units.
pub(crate) type Curve = crate::render3d::TfStops;

/// A starting curve over `lo..hi`: clear at the bottom, rising to solid at the top.
pub(crate) fn default_curve((lo, hi): (f32, f32)) -> Curve {
    let at = |t: f32| lo + (hi - lo) * t;
    [
        [at(0.0), 0.0],
        [at(0.35), 0.08],
        [at(0.65), 0.55],
        [at(1.0), 1.0],
    ]
    .into()
}

/// Phase H2's opacity curve (M3.5): a checkbox, and when on a small plot of its stops (value
/// across, opacity up) to drag. Two to eight stops: double-click the plot or "+" adds one on the
/// curve, right-click a stop or "−" removes one. Each stop stays between its neighbours, so the
/// curve never folds.
pub(crate) fn opacity_curve(
    ui: &mut egui::Ui,
    curve: &mut Option<Curve>,
    (lo, hi): (f32, f32),
    suffix: &str,
) {
    let mut on = curve.is_some();
    if ui
        .checkbox(&mut on, "Opacity curve")
        .on_hover_text(
            "Draw how see-through each value is: drag the stops, double-click to add one, \
             right-click one to remove it. Replaces the fixed ramp from the floor",
        )
        .changed()
    {
        *curve = on.then(|| default_curve((lo, hi)));
    }
    let Some(tf) = curve else {
        return;
    };
    let w = ui.available_width().clamp(160.0, 300.0);
    let (rect, plot_resp) = ui.allocate_exact_size(egui::vec2(w, 84.0), egui::Sense::click());
    let plot = rect.shrink(6.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    for f in [0.25, 0.5, 0.75] {
        let y = plot.bottom() - plot.height() * f;
        painter.hline(
            plot.x_range(),
            y,
            egui::Stroke::new(0.5, ui.visuals().weak_text_color().gamma_multiply(0.4)),
        );
    }
    let span = (hi - lo).max(f32::EPSILON);
    let to_screen = |p: [f32; 2]| {
        egui::pos2(
            plot.left() + (p[0] - lo) / span * plot.width(),
            plot.bottom() - p[1] * plot.height(),
        )
    };
    let value_at = |x: f32| lo + ((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * span;
    let id = ui.id().with("opacity_curve");
    let n = tf.len();
    let mut remove = None;
    for i in 0..n {
        let pts = tf.points_mut();
        let handle = egui::Rect::from_center_size(to_screen(pts[i]), egui::vec2(14.0, 14.0));
        let resp = ui.interact(handle, id.with(i), egui::Sense::click_and_drag());
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let left = if i > 0 { pts[i - 1][0] } else { lo };
                let right = if i + 1 < n { pts[i + 1][0] } else { hi };
                pts[i][0] = value_at(p.x).clamp(left, right);
                pts[i][1] = ((plot.bottom() - p.y) / plot.height()).clamp(0.0, 1.0);
            }
        }
        if resp.secondary_clicked() {
            remove = Some(i);
        }
        resp.on_hover_text(format!(
            "{:.1}{suffix} \u{2192} {:.0}% opaque (right-click removes)",
            pts[i][0],
            pts[i][1] * 100.0
        ));
    }
    if let Some(i) = remove {
        tf.remove(i);
    }
    if plot_resp.double_clicked() {
        if let Some(p) = plot_resp.interact_pointer_pos() {
            tf.insert(value_at(p.x));
        }
    }
    let pts = tf.points().to_vec();
    let accent = ui.visuals().selection.bg_fill;
    let line: Vec<egui::Pos2> = std::iter::once(egui::pos2(plot.left(), to_screen(pts[0]).y))
        .chain(pts.iter().map(|p| to_screen(*p)))
        .chain(std::iter::once(egui::pos2(
            plot.right(),
            to_screen(pts[pts.len() - 1]).y,
        )))
        .collect();
    painter.add(egui::Shape::line(line, egui::Stroke::new(2.0, accent)));
    for p in pts.iter() {
        painter.circle(
            to_screen(*p),
            4.5,
            accent,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
    }
    // Buttons too, for touch: a stop in the widest gap, or the last interior one removed.
    ui.horizontal(|ui| {
        let gap = pts
            .windows(2)
            .enumerate()
            .max_by(|a, b| (a.1[1][0] - a.1[0][0]).total_cmp(&(b.1[1][0] - b.1[0][0])))
            .map(|(i, w)| (i, (w[0][0] + w[1][0]) / 2.0));
        if ui
            .add_enabled(
                tf.len() < crate::render3d::MAX_STOPS,
                egui::Button::new("+"),
            )
            .on_hover_text("Add a stop in the widest gap")
            .clicked()
        {
            if let Some((_, v)) = gap {
                tf.insert(v);
            }
        }
        if ui
            .add_enabled(tf.len() > 2, egui::Button::new("\u{2212}"))
            .on_hover_text("Remove the last stop before the end")
            .clicked()
        {
            tf.remove(tf.len() - 2);
        }
        ui.weak(format!(
            "{} of {} stops \u{b7} {lo:.0}{suffix} \u{2192} {hi:.0}{suffix} across, clear to \
             solid up",
            tf.len(),
            crate::render3d::MAX_STOPS
        ));
    });
}

/// Colour stops beside the opacity curve (M3.5, 1008.md C2): a checkbox, and when on a bar of
/// the colours across the value range with a handle per stop. Drag a handle along the bar,
/// click it to pick its colour, double-click the bar (or "+") to add one with the colour already
/// there, right-click one (or "\u{2212}") to remove it. Only the volume's colour table changes;
/// `available` false says why there are none instead (a table indexed by speed, or inverted).
pub(crate) fn color_stops(
    ui: &mut egui::Ui,
    stops: &mut Option<crate::render3d::ColorStops>,
    (lo, hi): (f32, f32),
    suffix: &str,
    available: bool,
) {
    if !available {
        let why = "Colour stops: not offered here \u{2014} this volume's colours are indexed by \
                   speed or inverted, not by value";
        ui.add(egui::Label::new(egui::RichText::new(why).weak()).wrap());
        return;
    }
    let mut on = stops.is_some();
    if ui
        .checkbox(&mut on, "Colour stops")
        .on_hover_text(
            "Replace the palette's colours in this 3D volume: drag a stop along the bar, click \
             it for its colour, double-click the bar to add one, right-click one to remove it. \
             Values, probe and exports are unchanged",
        )
        .changed()
    {
        *stops = on.then(|| crate::render3d::ColorStops::default_for((lo, hi)));
    }
    let Some(cs) = stops else {
        return;
    };
    let w = ui.available_width().clamp(160.0, 300.0);
    let (rect, bar_resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
    let bar = egui::Rect::from_min_max(
        rect.min + egui::vec2(6.0, 4.0),
        egui::pos2(rect.max.x - 6.0, rect.min.y + 18.0),
    );
    let painter = ui.painter_at(rect);
    let span = (hi - lo).max(f32::EPSILON);
    let x_of = |v: f32| bar.left() + ((v - lo) / span).clamp(0.0, 1.0) * bar.width();
    let value_at = |x: f32| lo + ((x - bar.left()) / bar.width()).clamp(0.0, 1.0) * span;
    let cols = 48;
    for i in 0..cols {
        let x0 = bar.left() + bar.width() * i as f32 / cols as f32;
        let x1 = bar.left() + bar.width() * (i + 1) as f32 / cols as f32;
        let c = cs.color(value_at((x0 + x1) / 2.0));
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, bar.top()), egui::pos2(x1, bar.bottom())),
            0.0,
            egui::Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    let id = ui.id().with("color_stops");
    let n = cs.len();
    let mut remove = None;
    let mut pick = None;
    for i in 0..n {
        let pts = cs.points_mut();
        let x = x_of(pts[i].0);
        let handle =
            egui::Rect::from_center_size(egui::pos2(x, bar.bottom() + 5.0), egui::vec2(14.0, 14.0));
        let resp = ui.interact(handle, id.with(i), egui::Sense::click_and_drag());
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let left = if i > 0 { pts[i - 1].0 } else { lo };
                let right = if i + 1 < n { pts[i + 1].0 } else { hi };
                pts[i].0 = value_at(p.x).clamp(left, right);
            }
        }
        if resp.clicked() {
            pick = Some(i);
        }
        if resp.secondary_clicked() {
            remove = Some(i);
        }
        let c = pts[i].1;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, bar.bottom()),
                egui::pos2(x + 6.0, bar.bottom() + 10.0),
                egui::pos2(x - 6.0, bar.bottom() + 10.0),
            ],
            egui::Color32::from_rgb(c[0], c[1], c[2]),
            egui::Stroke::new(1.0, ui.visuals().strong_text_color()),
        ));
        resp.on_hover_text(format!(
            "{:.1}{suffix} (click for its colour, right-click removes)",
            pts[i].0
        ));
    }
    if let Some(i) = pick {
        ui.memory_mut(|m| m.data.insert_temp(id.with("picking"), i));
    }
    if let Some(i) = remove {
        cs.remove(i);
        ui.memory_mut(|m| m.data.remove::<usize>(id.with("picking")));
    }
    if bar_resp.double_clicked() {
        if let Some(p) = bar_resp.interact_pointer_pos() {
            cs.insert(value_at(p.x));
        }
    }
    ui.horizontal(|ui| {
        let picking: Option<usize> = ui
            .memory(|m| m.data.get_temp(id.with("picking")))
            .filter(|i| *i < cs.len());
        if let Some(i) = picking {
            let pts = cs.points_mut();
            ui.label(format!("{:.1}{suffix}", pts[i].0));
            ui.color_edit_button_srgb(&mut pts[i].1);
        }
        let gap = cs
            .points()
            .windows(2)
            .max_by(|a, b| (a[1].0 - a[0].0).total_cmp(&(b[1].0 - b[0].0)))
            .map(|w| (w[0].0 + w[1].0) / 2.0);
        if ui
            .add_enabled(
                cs.len() < crate::render3d::MAX_STOPS,
                egui::Button::new("+"),
            )
            .on_hover_text("Add a stop in the widest gap")
            .clicked()
        {
            if let Some(v) = gap {
                cs.insert(v);
            }
        }
        if ui
            .add_enabled(cs.len() > 2, egui::Button::new("\u{2212}"))
            .on_hover_text("Remove the last stop before the end")
            .clicked()
        {
            cs.remove(cs.len() - 2);
        }
        ui.weak(format!(
            "{} of {} stops",
            cs.len(),
            crate::render3d::MAX_STOPS
        ));
    });
}

/// Saved 3D looks (Phase H2's presets): pick one for this representation to set its floor,
/// ceiling and curve; or save what is set now under a name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn presets_row(
    ui: &mut egui::Ui,
    presets: &mut Vec<crate::settings::Volume3dPreset>,
    representation: &str,
    floor: &mut f32,
    denoise: &mut bool,
    ceiling: &mut Option<f32>,
    curve: &mut Option<Curve>,
    render: &mut crate::render3d::VolumeRender,
    colors: &mut Option<crate::render3d::ColorStops>,
    name_buf: &mut String,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mine: Vec<usize> = (0..presets.len())
            .filter(|i| presets[*i].representation == representation)
            .collect();
        egui::ComboBox::from_id_salt(("volume3d_preset", representation))
            .selected_text("Presets")
            .width(90.0)
            .show_ui(ui, |ui| {
                if mine.is_empty() {
                    ui.weak("None saved yet");
                }
                for i in &mine {
                    let p = &presets[*i];
                    if ui.selectable_label(false, &p.name).clicked() {
                        *floor = p.floor;
                        *denoise = true;
                        *ceiling = p.ceiling;
                        *curve = p.tf();
                        // Presets from before translucent rendering were drawn for MIP.
                        *render = p.render.unwrap_or_default();
                        // Presets from before colour stops keep the palette.
                        *colors = p
                            .colors
                            .as_deref()
                            .and_then(crate::render3d::ColorStops::from_saved);
                        changed = true;
                    }
                }
                if let Some(i) = mine.last() {
                    ui.separator();
                    if ui
                        .button(format!("Delete \u{201c}{}\u{201d}", presets[*i].name))
                        .clicked()
                    {
                        presets.remove(*i);
                        changed = true;
                    }
                }
            });
        ui.add(
            egui::TextEdit::singleline(name_buf)
                .hint_text("name")
                .desired_width(80.0),
        );
        if ui
            .add_enabled(!name_buf.trim().is_empty(), egui::Button::new("Save"))
            .on_hover_text(
                "Save this floor, ceiling, curve, colours and rendering for this 3D product",
            )
            .clicked()
        {
            let name = name_buf.trim().to_string();
            presets.retain(|p| !(p.representation == representation && p.name == name));
            let mut preset = crate::settings::Volume3dPreset {
                name,
                representation: representation.to_string(),
                floor: *floor,
                ceiling: *ceiling,
                curve: None,
                stops: None,
                render: Some(*render),
                colors: colors.as_ref().map(|c| c.to_saved()),
            };
            preset.set_tf(*curve);
            presets.push(preset);
            name_buf.clear();
            changed = true;
        }
    });
    changed
}

/// Phase H4: an extra vertical clip plane at any bearing, on top of the axis-aligned slab above —
/// the one way to cut into a storm along the angle it actually leans or approaches from rather
/// than only the box's own east-west/north-south faces. Shared with the main map's own "3D map"
/// Slice section (`app.rs`'s `map_3d_controls`), which raymarches the same kind of volume.
pub(crate) fn plane_controls(
    ui: &mut egui::Ui,
    plane: &mut Option<crate::render3d::VerticalPlane>,
) {
    let mut on = plane.is_some();
    if ui
        .checkbox(&mut on, "Vertical plane")
        .on_hover_text("Cut the volume with a plane at any angle, not just the box's own faces")
        .changed()
    {
        *plane = on.then_some(crate::render3d::VerticalPlane {
            bearing_deg: 0.0,
            offset: 0.0,
            thickness: None,
        });
    }
    if let Some(p) = plane {
        ui.horizontal(|ui| {
            ui.label("Bearing");
            crate::theme::slider(
                ui,
                egui::Slider::new(&mut p.bearing_deg, 0.0..=360.0)
                    .suffix("\u{b0}")
                    .custom_formatter(|v, _| format!("{v:.0}")),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Offset");
            crate::theme::slider(
                ui,
                egui::Slider::new(&mut p.offset, -1.0..=1.0).show_value(false),
            );
        })
        .response
        .on_hover_text(
            "Slides the plane through the volume; the side the bearing points toward is kept",
        );
        let mut slab = p.thickness.is_some();
        if ui
            .checkbox(&mut slab, "Slab")
            .on_hover_text(
                "Keep only a band straddling the plane instead of cutting one whole side away — \
                 two parallel planes with a gap rather than one",
            )
            .changed()
        {
            p.thickness = slab.then_some(0.1);
        }
        if let Some(t) = &mut p.thickness {
            ui.horizontal(|ui| {
                ui.label("Thickness");
                crate::theme::slider(ui, egui::Slider::new(t, 0.02..=1.0).show_value(false));
            });
        }
    }
}

/// Phase H4's last open item: a horizontal reference plane inside the 3D view showing where the
/// CAPPI window's altitude sits, since that window already slices the volume at that height as
/// its own separate 2D tool but nothing showed *where* until now. Shares the CAPPI window's own
/// altitude (`alt_km`) rather than keeping a second value here, so dragging its slider moves this
/// marker live. Shared with the main map's own "3D map" Slice section, same as `plane_controls`.
/// MIP / Translucent / Lit, shared by the 3D window and the 3D map (ROADMAP_PARITY M3.5).
pub(crate) fn render_mode_row(ui: &mut egui::Ui, render: &mut crate::render3d::VolumeRender) {
    ui.horizontal(|ui| {
        ui.label("Render");
        for mode in crate::render3d::VolumeRender::ALL {
            ui.selectable_value(render, mode, mode.label())
                .on_hover_text(mode.describe());
        }
    });
}

pub(crate) fn cappi_marker_controls(ui: &mut egui::Ui, on: &mut bool, alt_km: f32) {
    ui.checkbox(on, "CAPPI altitude").on_hover_text(format!(
        "Show a reference plane at the CAPPI window's altitude ({alt_km:.1} km)"
    ));
}

/// Show the 3D window. `pending` is a one-shot volume upload consumed by the first paint;
/// `n`/`nz` are the grid dimensions, `top_km` the volume's own vertical span (needed only to
/// place `cappi_alt_km`'s reference plane), and `range` the volume's `(value_min, value_max)`
/// dBZ span.
// ponytail: loose arguments rather than a params struct — every one of them is already a field
// on the caller, and a struct here would just be that call site written twice.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    st: &mut Volume3dState,
    pending: &mut Option<Volume3dUpload>,
    n: u32,
    nz: u32,
    top_km: f32,
    range: (f32, f32),
    cappi_alt_km: f32,
    drawer: &mut crate::ui::drawer::Drawer,
    degraded: bool,
) {
    let mut keep = *open;
    let Some(window) = drawer.page_sized(
        ctx,
        "3D Reflectivity",
        &mut keep,
        false,
        480.0,
        egui::Window::new("3D Reflectivity"),
    ) else {
        *open = keep;
        return;
    };
    window.show(ctx, |ui| {
        let size = ui.available_size();
        body(
            ui,
            st,
            pending,
            n,
            nz,
            top_km,
            range,
            cappi_alt_km,
            degraded,
            size,
        );
    });
    *open = keep;
}

/// The window's contents: the controls, then the orbitable view filling `view` (width, and the
/// height the view may take below the controls). Shared by the floating window and the
/// workstation's 3D volume tool window.
#[allow(clippy::too_many_arguments)]
pub fn body(
    ui: &mut egui::Ui,
    st: &mut Volume3dState,
    pending: &mut Option<Volume3dUpload>,
    n: u32,
    nz: u32,
    top_km: f32,
    range: (f32, f32),
    cappi_alt_km: f32,
    degraded: bool,
    view: egui::Vec2,
) {
    let ctx = ui.ctx().clone();
    let previous_selection = st.selected_elevs.clone();
    {
        let _ = source_status(ui, st);
        ui.weak("Drag to orbit · scroll to zoom");
        render_mode_row(ui, &mut st.render);
        ui.horizontal(|ui| {
            let mut on = st.threshold_dbz.is_finite();
            if ui
                .checkbox(&mut on, "Only above")
                .on_hover_text("Hide everything weaker, so cores stand alone")
                .changed()
            {
                st.threshold_dbz = if on { 45.0 } else { f32::NEG_INFINITY };
            }
            if on {
                let mut dbz = st.threshold_dbz;
                if ui
                    .add(egui::Slider::new(&mut dbz, range.0..=range.1).suffix(" dBZ"))
                    .changed()
                {
                    st.threshold_dbz = dbz;
                }
                if ui
                    .button("Hail core")
                    .on_hover_text("45 dBZ — the usual floor for a hail core")
                    .clicked()
                {
                    st.threshold_dbz = 45.0;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Quality");
            for (label, steps) in STEP_PRESETS {
                ui.selectable_value(&mut st.steps, steps, label)
                    .on_hover_text(format!("{steps} samples per pixel"));
            }
        });
        // Layer by layer, as on the 3D map: pick tilts and the view shows just their beams.
        if !st.layers.is_empty() {
            let layers = st.layers.clone();
            layers_section(
                ui,
                "volume3d_window_layers",
                &layers,
                &mut st.selected_elevs,
                "dBZ",
                "Click a tilt to show just its beam, as scanned — click more to compare several. None selected shows the whole interpolated volume.",
            );
            if !st.selected_elevs.is_empty() {
                ui.weak(format!(
                    "Selected {} tilt{}' beams (0.95° thick).",
                    st.selected_elevs.len(),
                    if st.selected_elevs.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
        }
        egui::CollapsingHeader::new("Slice")
            .default_open(false)
            .show(ui, |ui| {
                let [x0, x1, y0, y1, z0, z1] = &mut st.clip;
                axis_slice(ui, "E–W", x0, x1);
                axis_slice(ui, "N–S", y0, y1);
                axis_slice(ui, "Up ", z0, z1);
                if ui.button("Whole volume").clicked() {
                    st.clip = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
                }
                ui.separator();
                plane_controls(ui, &mut st.plane);
                cappi_marker_controls(ui, &mut st.cappi_marker, cappi_alt_km);
            });
        let size = egui::vec2(
            view.x.min(ui.available_width()),
            view.y.min(ui.available_height()).max(160.0),
        );
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::drag());
        if !st.current || previous_selection != st.selected_elevs {
            ui.painter()
                .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
            // Do not issue a callback: the GPU may still contain an older source or policy.
            // Preserve pending uploads until a matching grid has an actual paintable rectangle.
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if st.building {
                    "Preparing 3D grid…"
                } else {
                    "3D grid unavailable"
                },
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
            return;
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            st.az -= d.x * 0.4;
            st.el = (st.el + d.y * 0.3).clamp(2.0, 88.0);
        }
        // Wheel dollies the orbit camera; a trackpad pinch arrives as `zoom_delta` instead
        // (scale factor, >1 is fingers apart = closer) and has to move `dist` the other way.
        let (scroll, zoom) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if resp.hovered() {
            if scroll != 0.0 {
                st.dist = (st.dist - scroll * 0.003).clamp(1.3, 6.0);
            }
            if (zoom - 1.0).abs() > f32::EPSILON {
                st.dist = (st.dist / zoom).clamp(1.3, 6.0);
            }
        }
        let aspect = rect.width() / rect.height().max(1.0);
        // The threshold is a uniform, not a re-upload: dragging the slider never rebuilds the
        // texture.
        let view = View3d {
            tf: None,
            render: st.render,
            threshold_idx: if st.threshold_dbz.is_finite() {
                threshold_index(st.threshold_dbz, range)
            } else {
                2.0
            },
            ceiling_idx: 0.0,
            clip: st.clip,
            plane: st.plane,
            // The standalone orbit window is the reflectivity viewer; CC never reaches it, so it
            // has no anomaly ramp to carry.
            cc: [0.0; 4],
            cappi_km: st.cappi_marker.then_some(cappi_alt_km),
        };
        let uniform = orbit_uniform(
            st.az,
            st.el,
            st.dist,
            aspect,
            n,
            nz,
            st.half_km,
            top_km,
            effective_steps(st.steps, degraded),
            view,
        );
        // On the window's first frame egui may hand us a zero-area rect (auto-size pass);
        // the paint callback would be culled and the one-shot upload lost, leaving the view
        // black until reopened. Hold the upload until the rect is real.
        let upload = (rect.height() >= 1.0 && rect.width() >= 1.0)
            .then(|| pending.take())
            .flatten();
        let ppp = ctx.pixels_per_point();
        let target_px = crate::render3d::offscreen_px(
            [rect.width() * ppp, rect.height() * ppp],
            crate::render3d::raymarch_scale(cfg!(target_os = "android"), degraded),
            // A floating window never reaches the smallest texture limit wgpu guarantees.
            2048,
        );
        let cb = Volume3dCallback {
            upload,
            uniform,
            target_px,
        };
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    }
}

/// Kept separate from the raymarch callback so source/coverage controls can be reviewed even
/// on machines without a supported 3D texture device.
fn source_status(ui: &mut egui::Ui, st: &mut Volume3dState) -> Option<egui::Id> {
    let status = if st.building {
        "Preparing the selected scan; the previous grid is hidden."
    } else if !st.current {
        if st.frame.is_some() {
            "No matching 3D grid. Previous source details remain below."
        } else {
            "No matching 3D grid."
        }
    } else {
        "3D grid matches the selected scan revision."
    };
    ui.add(egui::Label::new(status).wrap());
    if let Some(error) = &st.error {
        ui.add(egui::Label::new(format!("Unavailable: {error}")).wrap());
        if ui.button("Retry 3D build").clicked() {
            st.retry = true;
        }
    }
    let Some((label, coverage)) = &st.frame else {
        return None;
    };
    let disclosure = egui::CollapsingHeader::new(if st.current {
        "Source and coverage"
    } else {
        "Previous source and coverage"
    })
    .id_salt("volume3d_source")
    .show(ui, |ui| {
        source_details(ui, label, coverage);
    });
    Some(disclosure.header_response.id)
}

fn source_details(ui: &mut egui::Ui, label: &VolumeFrameLabel, coverage: &TemporalCoverage) {
    ui.add(
        egui::Label::new(format!(
            "{} · {} · revision {}",
            label.site.as_deref().unwrap_or("Unknown site"),
            label.volume,
            label.revision
        ))
        .wrap(),
    );
    for line in coverage_lines(coverage) {
        ui.add(egui::Label::new(line).wrap());
    }
    for (name, value) in crate::ui::acquisition_inventory::contributor_rows(
        coverage
            .contributors
            .iter()
            .map(|sweep| sweep.native_passes.as_ref()),
        "input azimuth rows",
    ) {
        ui.add(egui::Label::new(format!("{name}: {value}")).wrap());
    }
    for (name, value) in crate::ui::acquisition_inventory::receipt_rows(label.acquisition.as_ref())
    {
        ui.add(egui::Label::new(format!("{name}: {value}")).wrap());
    }
}

fn coverage_lines(coverage: &TemporalCoverage) -> Vec<String> {
    let policy = match coverage.policy {
        TemporalPolicy::Continuous => "Continuous: older input rows retained",
        TemporalPolicy::StrictCurrent => "Strict current: older input rows excluded",
    };
    let interval = coverage.acquisition_range_ms().and_then(|(start, end)| {
        Some((
            chrono::DateTime::from_timestamp_millis(start)?,
            chrono::DateTime::from_timestamp_millis(end)?,
            end - start,
        ))
    });
    let clock = match interval {
        Some((start, end, span)) => format!(
            "Input: {} – {} UTC ({:.1} s)",
            start.format("%Y-%m-%d %H:%M:%S"),
            end.format("%H:%M:%S"),
            span as f64 / 1000.0
        ),
        None => "Input clocks unknown".into(),
    };
    vec![
        policy.into(),
        format!("{} contributing tilt(s)", coverage.contributors.len()),
        clock,
        format!(
            "Retained older rows: {}; excluded rows: {}",
            coverage.retained_older_rows(),
            coverage.excluded_rows()
        ),
        format!(
            "Unobserved rows: {} (not proven transport gaps)",
            coverage.unobserved_rows()
        ),
        format!(
            "Unknown input clocks: {} row(s)",
            coverage.unknown_time_rows()
        ),
        "Pass boundaries use source-time gap inference.".into(),
        "Complete columns are not established by the available tilts.".into(),
    ]
}

fn effective_steps(chosen: u32, degraded: bool) -> u32 {
    if degraded {
        chosen.min(96)
    } else {
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render3d::threshold_index;

    fn coverage(policy: TemporalPolicy) -> TemporalCoverage {
        let mut sweeps = [wxdata::level2::BinnedSweep {
            az_bins: 8,
            gate_count: 1,
            data: vec![120, 120, 120, 120, 120, 120, 0, 120],
            bin_time_ms: vec![
                1_700_000_120_000,
                1_700_000_121_000,
                1_700_000_122_000,
                1_700_000_123_000,
                1_700_000_000_000,
                1_700_000_001_000,
                0,
                0,
            ],
            elevation_deg: 0.5,
            ..Default::default()
        }];
        wxdata::level2::temporal::prepare(&mut sweeps, policy).unwrap()
    }

    fn frame() -> VolumeFrameLabel {
        VolumeFrameLabel {
            site: Some("DEMO".into()),
            volume: "controlled-input-20231114_221320".into(),
            revision: 7,
            acquisition: None,
        }
    }

    fn expand_source(ui: &mut egui::Ui, state: &mut Volume3dState) {
        // The header creates an internal vertical scope; use its real response ID rather than
        // predicting the parent's persistent ID. Later capture frames show the expanded body.
        let id = source_status(ui, state).expect("source disclosure");
        let mut disclosure =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
        disclosure.set_open(true);
        disclosure.store(ui.ctx());
        ui.ctx().animate_bool_with_time(id, true, 0.0);
    }

    #[test]
    fn volume_coverage_identifies_mixed_rows_and_unknown_clocks_without_volume_time_fallback() {
        let continuous = coverage_lines(&coverage(TemporalPolicy::Continuous));
        assert!(continuous.iter().any(|line| line.contains("123.0 s")));
        assert!(continuous.contains(&"Retained older rows: 2; excluded rows: 0".into()));
        let strict = coverage_lines(&coverage(TemporalPolicy::StrictCurrent));
        assert!(strict.iter().any(|line| line.contains("3.0 s")));
        assert!(strict.contains(&"Retained older rows: 0; excluded rows: 4".into()));
        assert!(strict
            .iter()
            .any(|line| line.contains("not proven transport gaps")));
        assert!(strict.iter().any(|line| line.contains("not established")));
        let mut unknown = coverage(TemporalPolicy::Continuous);
        unknown.contributors[0].used_start_ms = None;
        unknown.contributors[0].used_end_ms = None;
        assert!(coverage_lines(&unknown).contains(&"Input clocks unknown".into()));
    }

    #[test]
    fn standalone_receipt_summary_wraps_with_accepted_grid_coverage() {
        let (_, receipt) = crate::live_scan::acquisition_fixture("KPAH");
        let mut label = frame();
        label.acquisition = Some(receipt);
        let ctx = egui::Context::default();
        for width in [240.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    crate::ui::workstation::set_touch(ui.ctx(), width == 240.0);
                    source_details(ui, &label, &coverage(TemporalPolicy::Continuous));
                    assert!(ui.min_rect().right() <= width + 1.0);
                    assert!(ui.min_rect().bottom() < 900.0);
                },
            );
        }
    }

    #[test]
    fn unavailable_volume_omits_the_raymarch_callback_and_keeps_the_upload_pending() {
        let ctx = egui::Context::default();
        let mut state = Volume3dState::default();
        let mut pending = Some(Volume3dUpload {
            data: vec![0; 16],
            n: 2,
            nz: 2,
            lut: vec![0; 1024],
            half_km: 50.0,
            center_km: [0.0, 0.0],
            top_km: 20.0,
            outside: 0.0,
            value_range: None,
            lut_range: None,
        });
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                body(
                    ui,
                    &mut state,
                    &mut pending,
                    2,
                    2,
                    20.0,
                    (-20.0, 80.0),
                    1.0,
                    false,
                    egui::vec2(400.0, 300.0),
                );
            },
        );
        assert!(
            pending.is_some(),
            "no stale upload may be consumed by a hidden frame"
        );
        assert!(!output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_))));
        state.current = true;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                body(
                    ui,
                    &mut state,
                    &mut pending,
                    2,
                    2,
                    20.0,
                    (-20.0, 80.0),
                    1.0,
                    false,
                    egui::vec2(400.0, 300.0),
                );
            },
        );
        assert!(pending.is_none());
        assert!(output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_))));
    }

    #[test]
    fn source_details_wrap_in_narrow_volume_panels() {
        let ctx = egui::Context::default();
        for width in [240.0, 300.0] {
            for policy in [TemporalPolicy::Continuous, TemporalPolicy::StrictCurrent] {
                for capture_frame in 0..3 {
                    let _ = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 700.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            let mut state = Volume3dState {
                                current: true,
                                frame: Some((frame(), coverage(policy))),
                                ..Default::default()
                            };
                            expand_source(ui, &mut state);
                            assert!(ui.min_rect().width() <= width);
                            if capture_frame > 0 {
                                assert!(
                                    ui.min_rect().height() > 200.0,
                                    "the production disclosure must actually be expanded"
                                );
                            }
                        },
                    );
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the 3D colour-stop editor"]
    fn gpu_color_stops_editor_snapshot() {
        // The opacity curve and colour stops as the 3D map's Opacity curve section shows them,
        // for review (1008.md C2); and the explanation where colour stops are not offered.
        use crate::ui::workstation as ws;
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the editor");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m3.5");
        std::fs::create_dir_all(&destination).unwrap();
        let tokens = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let range = (-20.0, 80.0);
        for (name, available) in [("color-stops", true), ("color-stops-velocity", false)] {
            let mut curve = Some(default_curve(range));
            let mut colors = crate::render3d::ColorStops::new(&[
                (10.0, [40, 60, 170]),
                (35.0, [60, 190, 90]),
                (50.0, [235, 225, 60]),
                (65.0, [215, 40, 40]),
            ]);
            gpu.save(&destination.join(format!("{name}.png")), 360, 260, |ui| {
                ws::panel_frame(&tokens).show(ui, |ui| {
                    ws::style_scope(ui, &tokens);
                    ui.set_max_width(280.0);
                    opacity_curve(ui, &mut curve, range, " dBZ");
                    color_stops(ui, &mut colors, range, " dBZ", available);
                });
            })
            .unwrap();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes standalone 3D source and coverage controls"]
    fn gpu_volume_coverage_snapshots() {
        use crate::ui::workstation as ws;
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for source controls");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/volume-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let tokens = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for name in ["continuous", "strict", "pending", "unknown-error"] {
            for (width, touch) in [(240, true), (300, false)] {
                let policy = if name == "strict" || name == "pending" {
                    TemporalPolicy::StrictCurrent
                } else {
                    TemporalPolicy::Continuous
                };
                let mut source = coverage(policy);
                if name == "unknown-error" {
                    source.contributors[0].used_start_ms = None;
                    source.contributors[0].used_end_ms = None;
                }
                let mut state = Volume3dState {
                    current: name == "continuous" || name == "strict",
                    building: name == "pending",
                    error: (name == "unknown-error")
                        .then(|| "No reflectivity tilts match this selection".into()),
                    frame: Some((frame(), source.clone())),
                    ..Default::default()
                };
                gpu.save(
                    &destination.join(format!("{name}-{width}-touch-{touch}.png")),
                    width,
                    600,
                    |ui| {
                        ws::set_touch(ui.ctx(), touch);
                        ws::panel_frame(&tokens).show(ui, |ui| {
                            ws::style_scope(ui, &tokens);
                            ws::window_header(
                                ui,
                                &tokens,
                                egui_phosphor::regular::CUBE,
                                "3D volume",
                                None,
                                None,
                            );
                            // Review the actual disclosure body in its expanded state.
                            expand_source(ui, &mut state);
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn the_default_quality_is_one_of_the_presets() {
        // Otherwise no button reads as selected and the row looks broken on first open.
        let d = super::Volume3dState::default();
        assert!(super::STEP_PRESETS.iter().any(|&(_, s)| s == d.steps));
    }

    #[test]
    fn visual_guard_caps_ray_marching_without_changing_the_choice() {
        assert_eq!(super::effective_steps(256, true), 96);
        assert_eq!(super::effective_steps(96, true), 96);
        assert_eq!(super::effective_steps(256, false), 256);
    }

    #[test]
    fn dbz_maps_into_the_volume_index_range() {
        let range = (-30.0, 80.0);
        assert_eq!(threshold_index(-30.0, range), 2.0);
        assert_eq!(threshold_index(80.0, range), 255.0);
        // 45 dBZ sits where the hail-core preset should: well up the ramp, not at either end.
        let idx = threshold_index(45.0, range);
        assert!((170.0..=180.0).contains(&idx), "{idx}");
        // Out-of-range asks clamp rather than wrap.
        assert_eq!(threshold_index(-99.0, range), 2.0);
        assert_eq!(threshold_index(999.0, range), 255.0);
    }
}
