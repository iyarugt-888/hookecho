//! The ensemble's postage stamps (ROADMAP_NEW F7): the mean and every member of the field the
//! ensemble layer holds, side by side over the active pane's view, in the field's own colours.
//! Clicking one shows it on the map: a member on its own, or the mean.
//!
//! The stamps are coloured on the CPU from the members already held (nothing is fetched), and
//! made again only when the run, lead, field or view changes.

use super::*;

/// Stamp width in pixels; the height follows the view's shape.
const STAMP_W: usize = 132;

/// What a set of stamps was made from: field, run, lead, member count and the view's bounds
/// (rounded to a hundredth of a degree, so a sub-pixel pan does not redo them).
type StampKey = (
    wxdata::ensemble::EnsembleField,
    DateTime<Utc>,
    u16,
    usize,
    [i64; 4],
);

#[derive(Default)]
pub(crate) struct EnsembleStamps {
    pub open: bool,
    key: Option<StampKey>,
    /// The mean first, then each member.
    textures: Vec<egui::TextureHandle>,
}

impl HookEchoApp {
    pub(crate) fn ensemble_stamps_window(&mut self, ctx: &egui::Context) {
        if !self.ensemble_stamps.open {
            return;
        }
        let bounds = self.view_bounds();
        let (w, h) = stamp_size(bounds);
        if let Some(run) = &self.ensemble_run {
            let key = (
                self.ensemble.field,
                run.run,
                run.fcst_hour,
                run.members.len(),
                [bounds.0, bounds.1, bounds.2, bounds.3].map(|v| (v * 100.0).round() as i64),
            );
            if self.ensemble_stamps.key != Some(key) {
                let field = self.ensemble.field;
                let mean =
                    wxdata::ensemble::combine(&run.members, wxdata::ensemble::Statistic::Mean);
                let grids = mean.into_iter().chain(run.members.iter().cloned());
                self.ensemble_stamps.textures = grids
                    .enumerate()
                    .map(|(i, g)| {
                        let img = crate::ensemble_layer::stamp_image(&g, field, bounds, w, h);
                        ctx.load_texture(format!("ensemble_stamp_{i}"), img, Default::default())
                    })
                    .collect();
                self.ensemble_stamps.key = Some(key);
            }
        }
        let mut open = true;
        let mut pick: Option<Option<usize>> = None;
        let shown = self.ensemble.member;
        let showing_mean =
            shown.is_none() && self.ensemble.kind == crate::ensemble_layer::StatKind::Mean;
        egui::Window::new("Ensemble members")
            .open(&mut open)
            .default_width(4.0 * (STAMP_W as f32 + 10.0))
            .show(ctx, |ui| {
                let Some(run) = &self.ensemble_run else {
                    ui.weak(self.ensemble_status_line());
                    return;
                };
                ui.weak(format!(
                    "GEFS {} · run {} · valid {} · over the map view. Click one to show it.",
                    self.ensemble.field.label(),
                    run.run.format("%d %b %HZ"),
                    run.valid().format("%a %d %HZ"),
                ));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (i, tex) in self.ensemble_stamps.textures.iter().enumerate() {
                            // Slot 0 is the mean; slot i + 1 is member i.
                            let (label, this, on) = if i == 0 {
                                ("mean".to_string(), None, showing_mean)
                            } else {
                                let m = i - 1;
                                (
                                    crate::ensemble_layer::member_label(m),
                                    Some(m),
                                    shown == Some(m),
                                )
                            };
                            ui.vertical(|ui| {
                                let img =
                                    egui::Image::new((tex.id(), egui::vec2(w as f32, h as f32)));
                                let resp = ui.add(egui::Button::image(img).selected(on));
                                if resp.clicked() {
                                    pick = Some(this);
                                }
                                ui.small(label);
                            });
                        }
                    });
                });
            });
        self.ensemble_stamps.open = open;
        if let Some(choice) = pick {
            self.ensemble.member = choice;
            if choice.is_none() {
                self.ensemble.kind = crate::ensemble_layer::StatKind::Mean;
            }
        }
    }
}

/// A stamp's size for `bounds`: [`STAMP_W`] wide, as tall as the view is on the Mercator map
/// (kept between a third and the whole width).
fn stamp_size(bounds: (f64, f64, f64, f64)) -> (usize, usize) {
    use crate::render::mercator::lonlat_to_world;
    let (x0, y0) = lonlat_to_world(bounds.0, bounds.3);
    let (x1, y1) = lonlat_to_world(bounds.2, bounds.1);
    let aspect = ((y1 - y0).abs() / (x1 - x0).abs().max(1e-12)).clamp(0.33, 1.0);
    (STAMP_W, (STAMP_W as f64 * aspect).round() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_keeps_the_views_shape_within_limits() {
        let (w, h) = stamp_size((-105.0, 30.0, -85.0, 45.0));
        assert_eq!(w, STAMP_W);
        assert!(h > STAMP_W / 2 && h < STAMP_W, "{h}");
        // A thin strip is not a sliver.
        assert_eq!(stamp_size((-180.0, 40.0, 180.0, 41.0)).1, 44);
    }
}
