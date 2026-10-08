//! Screen captures (stills, clips, loop frames) and the per-frame field texture uploads.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Ask the viewport for an image, `dest` decides where it lands.
    ///
    /// With the share card on, the request waits two frames while [`share_card_footer`] draws the
    /// caption band, so the capture contains it.
    ///
    /// ponytail: the caption is drawn by egui into the frame rather than composited into the
    /// pixels afterwards — text layout, fonts and the theme are already solved here, and
    /// compositing them onto a raw RGBA buffer is a rasterizer we would have to grow.
    pub(crate) fn request_capture(&mut self, ctx: &egui::Context, dest: ShotDest) {
        if self.settings.share_card {
            // Two frames: one to lay the band out, one to be sure it is on screen when the
            // viewport grabs the image.
            self.share_card = Some((dest, 2));
            ctx.request_repaint();
        } else {
            self.screenshot_pending = Some(dest);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    /// Build the GPU upload for `layer` from its freshly-fetched grid, picking the value→index
    /// mapping and color LUT that suit the product's units.
    pub(crate) fn field_upload(
        &self,
        layer: crate::render::FieldLayer,
        f: &wxdata::mrms::MrmsField,
    ) -> crate::render::MrmsUpload {
        use crate::render::FieldLayer as FL;
        if layer
            .descriptor()
            .is_some_and(|field| field.default_palette == wxdata::field::PaletteId::Reflectivity)
        {
            return mrms_upload(f, self.palettes.table(Moment::Reflectivity));
        }
        match layer {
            // Mosaic + HRRR forecast are both dBZ → the reflectivity palette.
            FL::Mrms | FL::Mosaic | FL::Hrrr => {
                mrms_upload(f, self.palettes.table(Moment::Reflectivity))
            }
            // The accepted product's own table and range; without one, nothing is drawable.
            FL::UserColumnTrail => match self.column_trail_shown(self.active) {
                Some(t) => match t.range {
                    Some(range) => column_product::column_upload(f, &t.table, range),
                    None => field_upload_indexed(FL::UserColumnTrail, f),
                },
                None => field_upload_indexed(FL::UserColumnTrail, f),
            },
            FL::UserColumn => match self.column_accepted.as_deref() {
                Some(a) => column_product::column_upload(f, &a.table, a.range),
                None => field_upload_indexed(FL::UserColumn, f),
            },
            FL::ModelField => model_field::model_field_upload(f),
            other => field_upload_indexed(other, f),
        }
    }
}
