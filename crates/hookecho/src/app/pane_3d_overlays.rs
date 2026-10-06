//! What a pane draws over the tilted map in 3D mode: the MRMS surface, model isotherm sheets,
//! storm-cell columns and the radar beam guides. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7); each method takes the locals its block read, under the same names.

use super::*;

impl HookEchoApp {
    /// The MRMS surface (echo tops and the like), lifted to its height.
    pub(crate) fn paint_3d_mrms_surface(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.mrms_surface {
            use crate::render::FieldLayer as FL;
            let v = &self.views[idx];
            let layer = [
                FL::MrmsEchoTop18,
                FL::MrmsEchoTop30,
                FL::MrmsEchoTop50,
                FL::MrmsEchoTop60,
            ]
            .into_iter()
            .find(|l| v.fields_on.contains(l) && self.mrms_ready_for(idx, *l));
            if let (Some(layer), Some(ramp)) =
                (layer, layer.and_then(crate::render::field_ramps::ramp_for))
            {
                if let Some(grid) = self
                    .field_state_for(idx, layer)
                    .and_then(|f| f.grid.as_ref())
                {
                    // The lon/lat box the view covers, from its corners, capped at a regional size.
                    let corners = [
                        (0.0, 0.0),
                        (vp.0, 0.0),
                        (0.0, vp.1),
                        (vp.0, vp.1),
                        (vp.0 * 0.5, vp.1 * 0.5),
                    ]
                    .map(|p| {
                        let w = cam.screen_to_world(p, vp);
                        crate::render::mercator::world_to_lonlat(w.0, w.1)
                    });
                    let (clon, clat) = corners[4];
                    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                    for (lon, lat) in corners {
                        b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                    }
                    let b = [
                        b[0].max(clon - 6.0),
                        b[1].max(clat - 4.0),
                        b[2].min(clon + 6.0),
                        b[3].min(clat + 4.0),
                    ];
                    let lut = match &ramp.scale {
                        crate::render::field_ramps::FieldScale::Ramp { stops, .. } => {
                            crate::render::field_ramps::bake_ramp_lut(stops, 255)
                        }
                        _ => Vec::new(),
                    };
                    let (mesh, lines) = crate::render3d::height_surface_screen(
                        &cam,
                        vp,
                        prect.min,
                        grid,
                        b,
                        160,
                        v.map_3d.vertical_exaggeration as f64,
                        0.5,
                        |km| {
                            let i = ramp.index(km) as usize;
                            (i > 0 && lut.len() >= (i + 1) * 4)
                                .then(|| [lut[i * 4], lut[i * 4 + 1], lut[i * 4 + 2]])
                        },
                    );
                    painter.add(egui::Shape::mesh(mesh));
                    for l in lines {
                        painter.add(egui::Shape::line(
                            l,
                            egui::Stroke::new(0.6, egui::Color32::from_white_alpha(70)),
                        ));
                    }
                }
            }
        }
    }

    /// Model isotherm sheets at their heights.
    pub(crate) fn paint_3d_isotherms(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.model_isotherms {
            if let Some((_, (fields, run))) = self.model_isotherms.as_ref() {
                let v = &self.views[idx];
                let corners = [
                    (0.0, 0.0),
                    (vp.0, 0.0),
                    (0.0, vp.1),
                    (vp.0, vp.1),
                    (vp.0 * 0.5, vp.1 * 0.5),
                ]
                .map(|p| {
                    let w = cam.screen_to_world(p, vp);
                    crate::render::mercator::world_to_lonlat(w.0, w.1)
                });
                let (clon, clat) = corners[4];
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for (lon, lat) in corners {
                    b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                }
                let b = [
                    b[0].max(clon - 8.0),
                    b[1].max(clat - 6.0),
                    b[2].min(clon + 8.0),
                    b[3].min(clat + 6.0),
                ];
                // Highest first, so the lower, nearer surfaces paint over the ones above them.
                for (label, color, field) in fields.iter().rev() {
                    let (mesh, lines) = crate::render3d::height_surface_screen(
                        &cam,
                        vp,
                        prect.min,
                        field,
                        b,
                        // Smooth model fields: a coarse sheet reads the same and costs a third.
                        40,
                        v.map_3d.vertical_exaggeration as f64,
                        0.12,
                        |_| Some(*color),
                    );
                    // Where to name it: the surface point nearest the middle of the pane.
                    let centre = prect.center();
                    let anchor = mesh
                        .vertices
                        .iter()
                        .map(|v| v.pos)
                        .min_by(|a, b| a.distance(centre).total_cmp(&b.distance(centre)));
                    painter.add(egui::Shape::mesh(mesh));
                    let stroke = egui::Stroke::new(
                        1.2,
                        egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 220),
                    );
                    for l in &lines {
                        painter.extend(egui::Shape::dashed_line(l, stroke, 4.0, 4.0));
                    }
                    // Its name, so each sheet says what it is.
                    if let Some(p) = anchor {
                        painter.text(
                            p,
                            egui::Align2::CENTER_BOTTOM,
                            format!("{label} · {}Z run", run.format("%H")),
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_rgb(color[0], color[1], color[2]),
                        );
                    }
                }
            }
        }
    }

    /// Storm cells as columns standing to their tops.
    pub(crate) fn paint_3d_cell_columns(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.cell_columns {
            let v = &self.views[idx];
            let vex = v.map_3d.vertical_exaggeration as f64;
            let at = |p: (f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
            let scr = |lon: f64, lat: f64, km: f64| {
                crate::render3d::lonlat_alt_screen(&cam, vp, lon, lat, km, vex).map(at)
            };
            const KM_PER_KFT: f64 = 0.3048;
            for c in self.active_storm_cells() {
                let Some(top) = c.top_kft.map(|t| t as f64 * KM_PER_KFT) else {
                    continue;
                };
                let base = c
                    .base_kft
                    .filter(|_| !c.base_below)
                    .map_or(0.0, |b| b as f64 * KM_PER_KFT);
                let color = if c.tvs.is_some() {
                    egui::Color32::from_rgb(255, 70, 70)
                } else if c.meso.is_some() {
                    egui::Color32::from_rgb(255, 220, 60)
                } else {
                    egui::Color32::from_rgb(235, 235, 235)
                };
                let track: Vec<egui::Pos2> = c
                    .past_track
                    .iter()
                    .filter_map(|&(lon, lat)| scr(lon, lat, 0.0))
                    .collect();
                if track.len() >= 2 {
                    painter.add(egui::Shape::line(
                        track,
                        egui::Stroke::new(1.5, color.gamma_multiply(0.6)),
                    ));
                }
                let (Some(g), Some(b), Some(t)) = (
                    scr(c.lon, c.lat, 0.0),
                    scr(c.lon, c.lat, base),
                    scr(c.lon, c.lat, top),
                ) else {
                    continue;
                };
                // Ground to base dashed (below the cell), base to top solid.
                painter.extend(egui::Shape::dashed_line(
                    &[g, b],
                    egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
                    3.0,
                    3.0,
                ));
                painter.line_segment(
                    [b, t],
                    egui::Stroke::new(4.0, egui::Color32::from_black_alpha(110)),
                );
                painter.line_segment([b, t], egui::Stroke::new(2.0, color));
                if let Some(m) = c
                    .max_dbz_hgt_kft
                    .and_then(|h| scr(c.lon, c.lat, h as f64 * KM_PER_KFT))
                {
                    painter.circle(m, 4.0, color, egui::Stroke::new(1.0, egui::Color32::BLACK));
                }
                let mut label = format!("{} {:.0} kft", c.title, c.top_kft.unwrap_or(0.0));
                if let Some(dbz) = c.max_dbz {
                    label.push_str(&format!(" · {dbz:.0} dBZ"));
                }
                if c.tvs.is_some() {
                    label.push_str(" · TVS");
                } else if c.meso.is_some() {
                    label.push_str(" · meso");
                }
                painter.text(
                    t + egui::vec2(0.0, -4.0),
                    egui::Align2::CENTER_BOTTOM,
                    label,
                    egui::FontId::proportional(11.0),
                    color,
                );
            }
        }
    }

    /// Beam guides: where each tilt of the radar beam sits in height.
    pub(crate) fn paint_3d_beam_guides(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.beam_guides {
            let v = &self.views[idx];
            if let (Some(site), Some(vol)) = (
                v.site.as_deref().and_then(wxdata::sites::site_by_id),
                v.volume.as_ref(),
            ) {
                let (rlon, rlat) = (site.longitude as f64, site.latitude as f64);
                let ground_m = site.elevation_meters as f64;
                let antenna_m = ground_m + wxdata::towers::tower_m(site.id);
                // Beams point toward whatever the view is looking at.
                let (clon, clat) =
                    crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
                let bearing = crate::geo::bearing_deg([rlon, rlat], [clon, clat]);
                let g = crate::render3d::beam_guides(
                    &cam,
                    vp,
                    rlon,
                    rlat,
                    ground_m,
                    antenna_m,
                    v.map_3d.vertical_exaggeration as f64,
                    v.map_3d.beam_rise as f64,
                    &vol.elevations,
                    bearing,
                    230.0,
                );
                let at = |p: &(f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
                let n = vol.elevations.len().max(2) as f32 - 1.0;
                for (tilt, ring) in &g.rings {
                    // Low tilts cyan, high tilts magenta.
                    let t = (*tilt as f32 / n).clamp(0.0, 1.0);
                    let c = egui::Color32::from_rgba_unmultiplied(
                        (80.0 + 170.0 * t) as u8,
                        (220.0 - 150.0 * t) as u8,
                        240,
                        110,
                    );
                    let pts: Vec<egui::Pos2> = ring.iter().map(at).collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, c)));
                }
                for (edge, line) in &g.beams {
                    let pts: Vec<egui::Pos2> = line.iter().map(at).collect();
                    if *edge {
                        painter.extend(egui::Shape::dashed_line(
                            &pts,
                            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(150)),
                            4.0,
                            4.0,
                        ));
                    } else {
                        painter.add(egui::Shape::line(
                            pts,
                            egui::Stroke::new(1.8, egui::Color32::WHITE),
                        ));
                    }
                }
                if let Some([a, b]) = g.mast {
                    painter.line_segment(
                        [at(&a), at(&b)],
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                    );
                    painter.circle_filled(at(&b), 3.5, egui::Color32::WHITE);
                }
            }
        }
    }

    /// The satellite cloud-top surface, lifted to its height.
    pub(crate) fn paint_3d_cloud_tops(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.cloud_top_surface {
            if let Some((_, field)) = self.cloud_top.as_ref() {
                let v = &self.views[idx];
                let corners = [
                    (0.0, 0.0),
                    (vp.0, 0.0),
                    (0.0, vp.1),
                    (vp.0, vp.1),
                    (vp.0 * 0.5, vp.1 * 0.5),
                ]
                .map(|p| {
                    let w = cam.screen_to_world(p, vp);
                    crate::render::mercator::world_to_lonlat(w.0, w.1)
                });
                let (clon, clat) = corners[4];
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for (lon, lat) in corners {
                    b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                }
                let b = [
                    b[0].max(clon - 8.0),
                    b[1].max(clat - 6.0),
                    b[2].min(clon + 8.0),
                    b[3].min(clat + 6.0),
                ];
                let (mesh, _) = crate::render3d::height_surface_screen(
                    &cam,
                    vp,
                    prect.min,
                    field,
                    b,
                    120,
                    v.map_3d.vertical_exaggeration as f64,
                    0.35,
                    |km| {
                        let t = (km / 15.0).clamp(0.0, 1.0);
                        let lerp = |a: f32, b: f32| (a + t * (b - a)) as u8;
                        Some([lerp(120.0, 250.0), lerp(130.0, 252.0), lerp(150.0, 255.0)])
                    },
                );
                painter.add(egui::Shape::mesh(mesh));
            }
        }
    }
}
