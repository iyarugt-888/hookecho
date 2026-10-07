//! What a river covers at a chosen stage, drawn on the map: a gauge card's record crest, a crest
//! from its history, the current level or the forecast crest, clicked, shows NOAA's mapped
//! inundation for that stage, filled in the flood category's color, with what the forecast office
//! says happens there.
//!
//! The polygons come from the stage-based CatFIM library ([`wxdata::flood_fim`]): the highest
//! mapped stage at or below the one asked for, so the map never shows more water than that stage
//! reaches. A stage above everything mapped shows the most the library holds, and says so. A
//! gauge without maps, or a stage below the lowest map, still gets its category and impact
//! statement in the card. Each gauge's stage listing and the last few polygons are kept, so
//! stepping between crests at one gauge asks the service only for polygons it has not sent yet.
//!
//! A polygon is tessellated once, off the UI thread, in meters around its first point (lyon's
//! tolerances are made for screen-sized numbers, not map units in `[0, 1]`). Its vertices are kept
//! in world coordinates, so a frame only projects them.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use egui::{Color32, Mesh, Pos2, RichText};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use wxdata::flood_fim::{MappedStage, Pick};
use wxdata::river::{FloodCat, Impact};

use crate::render::mercator::{lonlat_to_world, Camera};
use crate::ui::gauge_card::{cat_color, cat_label};

/// How many polygons to keep for stepping back and forth between crests.
const KEEP_AREAS: usize = 8;

/// A stage on a gauge to show on the map.
#[derive(Clone, Debug)]
pub struct Ask {
    pub lid: String,
    pub stage_ft: f64,
    /// What the stage is, for the map and the card: "Record crest (Mar 3, 1990)".
    pub label: String,
    /// The gauge's category at this stage.
    pub cat: FloodCat,
    /// What the forecast office says happens at this stage, if anything.
    pub impact: Option<Impact>,
    /// The gauge, as (latitude, longitude).
    pub at: (f64, f64),
}

impl Ask {
    fn same(&self, lid: &str, stage_ft: f64) -> bool {
        self.lid == lid && (self.stage_ft - stage_ft).abs() < 0.005
    }
}

/// A tessellated inundation polygon, in world coordinates.
pub struct Area {
    verts: Vec<(f64, f64)>,
    indices: Vec<u32>,
    rings: Vec<Vec<(f64, f64)>>,
    /// West, south, east, north, in degrees.
    pub bounds: [f64; 4],
}

impl Area {
    /// Tessellate rings of (longitude, latitude). `None` when there is nothing with an area.
    pub fn new(rings: &[Vec<(f64, f64)>]) -> Option<Area> {
        let first = rings.iter().find(|r| r.len() >= 3)?[0];
        let (lon0, lat0) = first;
        let kx = 111_320.0 * lat0.to_radians().cos();
        let ky = 110_540.0;
        let mut b = Path::builder();
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for ring in rings.iter().filter(|r| r.len() >= 3) {
            let local = |&(lon, lat): &(f64, f64)| {
                point(((lon - lon0) * kx) as f32, ((lat - lat0) * ky) as f32)
            };
            b.begin(local(&ring[0]));
            for p in &ring[1..] {
                b.line_to(local(p));
            }
            b.close();
            for &(lon, lat) in ring {
                bounds = [
                    bounds[0].min(lon),
                    bounds[1].min(lat),
                    bounds[2].max(lon),
                    bounds[3].max(lat),
                ];
            }
        }
        let mut buf: VertexBuffers<(f64, f64), u32> = VertexBuffers::new();
        FillTessellator::new()
            .tessellate_path(
                &b.build(),
                &FillOptions::default().with_fill_rule(FillRule::NonZero),
                &mut BuffersBuilder::new(&mut buf, |v: FillVertex| {
                    let p = v.position();
                    lonlat_to_world(lon0 + p.x as f64 / kx, lat0 + p.y as f64 / ky)
                }),
            )
            .ok()?;
        if buf.indices.is_empty() {
            return None;
        }
        Some(Area {
            verts: buf.vertices,
            indices: buf.indices,
            rings: rings
                .iter()
                .filter(|r| r.len() >= 3)
                .map(|r| {
                    r.iter()
                        .map(|&(lon, lat)| lonlat_to_world(lon, lat))
                        .collect()
                })
                .collect(),
            bounds,
        })
    }
}

/// Where a request stands.
#[derive(Clone)]
pub enum Shown {
    Loading,
    Area {
        mapped: MappedStage,
        capped: bool,
        area: Arc<Area>,
    },
    Below {
        lowest_ft: f64,
    },
    Unmapped,
    Failed(String),
}

type Landed = (u64, String, Option<Vec<MappedStage>>, Shown);

/// The one stage shown on the map, and the caches behind it.
pub struct FloodImpact {
    ask: Option<Ask>,
    shown: Shown,
    /// Bumped by each request, so an answer to an older one is cached but not shown.
    seq: u64,
    libraries: HashMap<String, Vec<MappedStage>>,
    areas: Vec<((String, i64), Arc<Area>)>,
    /// Bounds the map should move to, set when a polygon first lands.
    fit: Option<[f64; 4]>,
    tx: Sender<Landed>,
    rx: Receiver<Landed>,
}

impl Default for FloodImpact {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            ask: None,
            shown: Shown::Loading,
            seq: 0,
            libraries: HashMap::new(),
            areas: Vec::new(),
            fit: None,
            tx,
            rx,
        }
    }
}

fn area_key(lid: &str, stage_ft: f64) -> (String, i64) {
    (lid.to_ascii_lowercase(), (stage_ft * 100.0).round() as i64)
}

impl FloodImpact {
    /// Whether this stage at this gauge is the one on the map.
    pub fn is_shown(&self, lid: &str, stage_ft: f64) -> bool {
        self.ask.as_ref().is_some_and(|a| a.same(lid, stage_ft))
    }

    /// What is on the map, if anything.
    pub fn current(&self) -> Option<(&Ask, &Shown)> {
        self.ask.as_ref().map(|a| (a, &self.shown))
    }

    pub fn clear(&mut self) {
        self.ask = None;
        self.fit = None;
    }

    /// The bounds to move the map to, once, after a polygon lands.
    pub fn take_fit(&mut self) -> Option<[f64; 4]> {
        self.fit.take()
    }

    /// Show `ask` on the map, or take it off when it is already there.
    pub fn toggle(
        &mut self,
        ask: Ask,
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
        ctx: &egui::Context,
    ) {
        if self.is_shown(&ask.lid, ask.stage_ft) {
            self.clear();
            return;
        }
        self.seq += 1;
        let (lid, stage_ft) = (ask.lid.clone(), ask.stage_ft);
        self.ask = Some(ask);
        self.fit = None;
        let library = self.libraries.get(&lid.to_ascii_lowercase()).cloned();
        if let Some(lib) = &library {
            match wxdata::flood_fim::pick(lib, stage_ft) {
                Pick::Mapped { stage, capped } => {
                    let key = area_key(&lid, stage.stage_ft);
                    if let Some((_, area)) = self.areas.iter().find(|(k, _)| *k == key) {
                        self.fit = Some(area.bounds);
                        self.shown = Shown::Area {
                            mapped: stage,
                            capped,
                            area: area.clone(),
                        };
                        return;
                    }
                }
                Pick::Below { lowest_ft } => {
                    self.shown = Shown::Below { lowest_ft };
                    return;
                }
                Pick::Unmapped => {
                    self.shown = Shown::Unmapped;
                    return;
                }
            }
        }
        self.shown = Shown::Loading;
        let (tx, http, ctx, seq) = (self.tx.clone(), http.clone(), ctx.clone(), self.seq);
        spawner.spawn(async move {
            let (fetched, lib) = match library {
                Some(l) => (None, Ok(l)),
                None => match wxdata::flood_fim::fetch_library(&http, &lid).await {
                    Ok(l) => (Some(l.clone()), Ok(l)),
                    Err(e) => (None, Err(e.to_string())),
                },
            };
            let shown = match lib {
                Err(e) => Shown::Failed(e),
                Ok(lib) => match wxdata::flood_fim::pick(&lib, stage_ft) {
                    Pick::Mapped { stage, capped } => {
                        match wxdata::flood_fim::fetch_inundation(&http, &lid, &stage).await {
                            Ok(rings) => match Area::new(&rings) {
                                Some(area) => Shown::Area {
                                    mapped: stage,
                                    capped,
                                    area: Arc::new(area),
                                },
                                None => Shown::Failed("the map for this stage is empty".into()),
                            },
                            Err(e) => Shown::Failed(e.to_string()),
                        }
                    }
                    Pick::Below { lowest_ft } => Shown::Below { lowest_ft },
                    Pick::Unmapped => Shown::Unmapped,
                },
            };
            let _ = tx.send((seq, lid, fetched, shown));
            ctx.request_repaint();
        });
    }

    /// Land finished requests: cache what came back, and show the newest.
    pub fn land(&mut self) {
        for (seq, lid, library, shown) in self.rx.try_iter() {
            if let Some(l) = library {
                self.libraries.insert(lid.to_ascii_lowercase(), l);
            }
            if let Shown::Area { mapped, area, .. } = &shown {
                let key = area_key(&lid, mapped.stage_ft);
                self.areas.retain(|(k, _)| *k != key);
                self.areas.push((key, area.clone()));
                if self.areas.len() > KEEP_AREAS {
                    self.areas.remove(0);
                }
            }
            if seq == self.seq && self.ask.is_some() {
                if let Shown::Area { area, .. } = &shown {
                    self.fit = Some(area.bounds);
                }
                self.shown = shown;
            }
        }
    }

    /// The fill and edge colors for the stage on the map.
    fn colors(cat: FloodCat) -> (Color32, Color32) {
        let c = match cat {
            FloodCat::NoFlooding | FloodCat::Unknown => Color32::from_rgb(60, 140, 255),
            c => cat_color(c),
        };
        (Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 105), c)
    }

    /// Draw the shown stage's inundation, its label at the gauge, and, under the pointer, what
    /// happens there.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: Camera,
        vp: (f32, f32),
        response: &egui::Response,
    ) {
        let Some(ask) = &self.ask else {
            return;
        };
        let to_screen = |w: (f64, f64)| {
            let (x, y) = cam.world_to_screen(w, vp);
            Pos2::new(prect.left() + x, prect.top() + y)
        };
        let (fill, edge) = Self::colors(ask.cat);
        let mut inside = false;
        if let Shown::Area { area, .. } = &self.shown {
            let [w, s, e, n] = area.bounds;
            let (a, b) = (
                to_screen(lonlat_to_world(w, n)),
                to_screen(lonlat_to_world(e, s)),
            );
            let on_screen = egui::Rect::from_two_pos(a, b).intersects(prect);
            if on_screen || cam.is_3d() {
                let mut mesh = Mesh::default();
                for &v in &area.verts {
                    mesh.colored_vertex(to_screen(v), fill);
                }
                mesh.indices = area.indices.clone();
                if let Some(hp) = response.hover_pos() {
                    inside = covers(&mesh, hp);
                }
                painter.add(egui::Shape::mesh(mesh));
                for ring in &area.rings {
                    let pts: Vec<Pos2> = ring.iter().map(|&w| to_screen(w)).collect();
                    painter.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.2, edge)));
                }
            }
        }
        // The stage at the gauge, under its marker.
        let g = to_screen(lonlat_to_world(ask.at.1, ask.at.0));
        if prect.contains(g) {
            let text = format!("{:.1} ft · {}", ask.stage_ft, ask.label);
            let galley =
                painter.layout_no_wrap(text, egui::FontId::proportional(11.5), Color32::WHITE);
            let r = egui::Rect::from_min_size(
                g + egui::vec2(-galley.size().x / 2.0, 12.0),
                galley.size(),
            )
            .expand2(egui::vec2(5.0, 2.0));
            painter.rect_filled(r, 0.0, Color32::from_black_alpha(190));
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, edge),
                egui::StrokeKind::Inside,
            );
            painter.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
            inside |= response.hover_pos().is_some_and(|hp| r.contains(hp));
        }
        if inside {
            let mut tip = format!(
                "{} at {:.1} ft — {}",
                ask.label,
                ask.stage_ft,
                cat_label(ask.cat)
            );
            if let Shown::Area { mapped, capped, .. } = &self.shown {
                tip.push_str(&mapped_note(mapped, *capped, ask.stage_ft));
            }
            if let Some(i) = &ask.impact {
                tip.push_str(&format!("\n\nAt {:.1} ft: {}", i.stage_ft, i.statement));
            }
            response.clone().show_tooltip_text(tip);
        }
    }

    /// The card's line about what is on the map for this gauge: what stage, how it was mapped,
    /// and what happens there. True when the reader asked to take it off the map.
    pub fn card_status(&self, ui: &mut egui::Ui, lid: &str) -> bool {
        let Some(ask) = self.ask.as_ref().filter(|a| a.lid == lid) else {
            return false;
        };
        let mut clear = false;
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(egui::Stroke::new(1.0, Self::colors(ask.cat).1))
            .corner_radius(0)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "On the map: {} at {:.1} ft",
                            ask.label, ask.stage_ft
                        ))
                        .size(11.5)
                        .strong(),
                    );
                    let cat = cat_label(ask.cat);
                    if !cat.is_empty() {
                        ui.label(RichText::new(cat).size(11.0).color(Self::colors(ask.cat).1));
                    }
                    clear = ui
                        .small_button("Clear")
                        .on_hover_text("Take this stage off the map")
                        .clicked();
                });
                let note = match &self.shown {
                    Shown::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Fetching the inundation map…").size(11.0));
                        });
                        None
                    }
                    Shown::Area { mapped, capped, .. } => Some(format!(
                        "The shaded area is NOAA's mapped flooding.{}",
                        mapped_note(mapped, *capped, ask.stage_ft).replace('\n', " ")
                    )),
                    Shown::Below { lowest_ft } => Some(format!(
                        "NOAA maps no flooding here below {lowest_ft:.1} ft."
                    )),
                    Shown::Unmapped => Some(
                        "NOAA has no inundation map for this gauge; the impacts below are the \
                         forecast office's."
                            .into(),
                    ),
                    Shown::Failed(e) => Some(format!("Could not fetch the inundation map: {e}")),
                };
                if let Some(n) = note {
                    ui.label(RichText::new(n).size(10.5).weak());
                }
                match &ask.impact {
                    Some(i) => {
                        ui.label(
                            RichText::new(format!("At {:.1} ft:", i.stage_ft))
                                .size(11.0)
                                .strong(),
                        );
                        ui.label(RichText::new(&i.statement).size(11.0));
                    }
                    None => {
                        ui.label(
                            RichText::new("No impact statement covers this stage.")
                                .size(10.5)
                                .weak(),
                        );
                    }
                }
            });
        clear
    }
}

/// How the polygon relates to the stage asked for, for a tooltip or the card.
fn mapped_note(mapped: &MappedStage, capped: bool, asked_ft: f64) -> String {
    if capped {
        format!(
            "\nMapped to {:.1} ft, the highest stage NOAA maps here; {asked_ft:.1} ft floods more.",
            mapped.stage_ft
        )
    } else if (mapped.stage_ft - asked_ft).abs() >= 0.05 {
        format!(
            "\nMapped at {:.1} ft, the nearest map below.",
            mapped.stage_ft
        )
    } else {
        String::new()
    }
}

/// Whether `p` is inside any triangle of `mesh`.
fn covers(mesh: &Mesh, p: Pos2) -> bool {
    mesh.indices.as_chunks::<3>().0.iter().any(|t| {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize].pos);
        let s = |p1: Pos2, p2: Pos2, p3: Pos2| {
            (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
        };
        let (d1, d2, d3) = (s(p, a, b), s(p, b, c), s(p, c, a));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    })
}

/// The camera that shows `bounds` (west, south, east, north) in a `size` pane, a little inside
/// its edges, kept between town scale and street scale.
pub fn fit_camera(cam: &Camera, bounds: [f64; 4], size: egui::Vec2) -> Camera {
    let [w, s, e, n] = bounds;
    let (x0, y0) = lonlat_to_world(w, n);
    let (x1, y1) = lonlat_to_world(e, s);
    let (dx, dy) = ((x1 - x0).abs().max(1e-9), (y1 - y0).abs().max(1e-9));
    let fit = (size.x as f64 * 0.8 / (dx * 256.0)).min(size.y as f64 * 0.8 / (dy * 256.0));
    Camera {
        center: ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        zoom: fit.log2().clamp(9.0, 15.0),
        ..*cam
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lon: f64, lat: f64, d: f64) -> Vec<(f64, f64)> {
        // Clockwise, as Esri writes an outer ring.
        vec![
            (lon, lat),
            (lon, lat + d),
            (lon + d, lat + d),
            (lon + d, lat),
            (lon, lat),
        ]
    }

    #[test]
    fn a_hole_in_the_flooding_stays_dry() {
        let outer = square(-84.05, 32.28, 0.04);
        let mut hole = square(-84.04, 32.29, 0.01);
        hole.reverse(); // counterclockwise: a hole
        let area = Area::new(&[outer, hole]).unwrap();
        for (got, want) in area.bounds.iter().zip([-84.05, 32.28, -84.01, 32.32]) {
            assert!((got - want).abs() < 1e-9, "{:?}", area.bounds);
        }
        let cam = Camera::at_lonlat(-84.03, 32.30, 12.0);
        let vp = (800.0, 600.0);
        let mut mesh = Mesh::default();
        for &v in &area.verts {
            let (x, y) = cam.world_to_screen(v, vp);
            mesh.colored_vertex(Pos2::new(x, y), Color32::WHITE);
        }
        mesh.indices = area.indices.clone();
        let at = |lon: f64, lat: f64| {
            let (x, y) = cam.world_to_screen(lonlat_to_world(lon, lat), vp);
            Pos2::new(x, y)
        };
        assert!(covers(&mesh, at(-84.045, 32.315)), "inside the ring is wet");
        assert!(!covers(&mesh, at(-84.035, 32.295)), "the hole is dry");
        assert!(!covers(&mesh, at(-84.06, 32.30)), "outside is dry");
        assert!(Area::new(&[vec![(0.0, 0.0), (1.0, 1.0)]]).is_none());
    }

    #[test]
    fn the_map_fits_the_flooded_area() {
        let cam = Camera::at_lonlat(-90.0, 35.0, 6.0);
        let fitted = fit_camera(
            &cam,
            [-84.05, 32.28, -84.01, 32.32],
            egui::vec2(800.0, 600.0),
        );
        assert!(fitted.zoom > 11.0 && fitted.zoom < 14.0, "{}", fitted.zoom);
        let (x, y) = fitted.world_to_screen(lonlat_to_world(-84.03, 32.30), (800.0, 600.0));
        assert!((x - 400.0).abs() < 2.0 && (y - 300.0).abs() < 2.0);
        // A whole river reach still fits, zoomed out, but not past town scale.
        let wide = fit_camera(&cam, [-86.0, 30.0, -82.0, 34.0], egui::vec2(800.0, 600.0));
        assert_eq!(wide.zoom, 9.0);
    }
    /// The Ogeechee River at Midville, GA (MDVG1) at a major stage, from the live library, drawn
    /// as the map draws it beside the card's line about it, for visual review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu + network: writes the flood impact map for visual review"]
    fn gpu_flood_impact_review() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let http = reqwest::Client::new();
        let (stage, rings) = rt.block_on(async {
            let lib = wxdata::flood_fim::fetch_library(&http, "MDVG1")
                .await
                .unwrap();
            let Pick::Mapped { stage, .. } = wxdata::flood_fim::pick(&lib, 15.4) else {
                panic!("MDVG1 is mapped")
            };
            let rings = wxdata::flood_fim::fetch_inundation(&http, "MDVG1", &stage)
                .await
                .unwrap();
            (stage, rings)
        });
        let area = Arc::new(Area::new(&rings).unwrap());
        let mut impact = FloodImpact::default();
        impact.ask = Some(Ask {
            lid: "MDVG1".into(),
            stage_ft: 15.4,
            label: "Record crest (Oct 1, 1929)".into(),
            cat: FloodCat::Major,
            impact: Some(Impact {
                stage_ft: 15.0,
                statement: "Major lowland flooding; county roads near the river close.".into(),
            }),
            at: (32.82, -82.23),
        });
        impact.shown = Shown::Area {
            mapped: stage,
            capped: false,
            area: area.clone(),
        };
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for UI review");
        let destination =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-review");
        std::fs::create_dir_all(&destination).unwrap();
        gpu.save(&destination.join("flood-impact.png"), 900, 560, |ui| {
            ui.horizontal_top(|ui| {
                let (resp, painter) =
                    ui.allocate_painter(egui::vec2(560.0, 540.0), egui::Sense::hover());
                let r = resp.rect;
                painter.rect_filled(r, 0.0, Color32::from_rgb(28, 32, 38));
                let cam = fit_camera(&Camera::at_lonlat(-82.2, 32.8, 6.0), area.bounds, r.size());
                impact.paint(&painter, r, cam, (r.width(), r.height()), &resp);
                ui.vertical(|ui| {
                    ui.set_width(320.0);
                    impact.card_status(ui, "MDVG1");
                });
            });
        })
        .unwrap();
    }
}
