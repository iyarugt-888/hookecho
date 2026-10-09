//! CPU tessellation of overlay features into GPU-ready triangles.
//!
//! Fills and outlines of the lon/lat [`GeoFeature`] polygons are projected to world space
//! and tessellated with lyon. Stroke width is set in world units for the current zoom, so
//! the app rebuilds this when the zoom bucket changes (feature counts are small).

use crate::render::mercator::lonlat_to_world;
use crate::render::OverlayVertex;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};
use wxdata::overlay::{FeatureKind, GeoFeature};
use wxdata::placefile::{PlaceItem, PlaceKind};

/// Tessellated overlay geometry ready for a single vertex+index draw.
#[derive(Default)]
pub struct OverlayGeom {
    pub vertices: Vec<OverlayVertex>,
    pub indices: Vec<u32>,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn color(rgba: [u8; 4]) -> [f32; 4] {
    [
        srgb_to_linear(rgba[0]),
        srgb_to_linear(rgba[1]),
        srgb_to_linear(rgba[2]),
        rgba[3] as f32 / 255.0,
    ]
}

fn feature_stroke_px(kind: FeatureKind, imported_stroke_px: f32) -> f32 {
    if kind == FeatureKind::Imported {
        imported_stroke_px.clamp(0.5, 8.0)
    } else if kind == FeatureKind::Boundary {
        1.0
    } else {
        1.6
    }
}

/// Build tessellated fills + outlines for `features` at `zoom` (normal theme).
pub fn build(features: &[GeoFeature], zoom: f64) -> OverlayGeom {
    build_with_theme(features, zoom, crate::settings::Theme::DearImGui)
}

/// Build tessellated fills + outlines for `features` at `zoom`, scaled by `theme`.
///
/// A high-contrast scheme (none is offered at present; see `theme::is_high_contrast`) doubles the outline width and boosts fill/stroke
/// alphas via `crate::theme` so warning polygons remain legible against both imagery and
/// bright radar — driven from `theme.rs`, not hardcoded in the overlay.
pub fn build_with_theme(
    features: &[GeoFeature],
    zoom: f64,
    theme: crate::settings::Theme,
) -> OverlayGeom {
    build_with_theme_and_imported_width(features, zoom, theme, 1.6, true)
}

/// Theme-aware overlay build with one imported-GIS outline width for every imported feature.
/// Official products retain the established 1.6 px edge.
pub fn build_with_theme_and_imported_width(
    features: &[GeoFeature],
    zoom: f64,
    theme: crate::settings::Theme,
    imported_stroke_px: f32,
    show_imported: bool,
) -> OverlayGeom {
    let px = vec![show_imported.then_some(imported_stroke_px.into()); features.len()];
    build_layered(features, zoom, theme, &px)
}

/// How an imported layer outlines its polygons: its width and, when not solid, its dash's on
/// and off lengths, all in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImportedStroke {
    pub width_px: f32,
    pub dash_px: Option<(f32, f32)>,
}

impl From<f32> for ImportedStroke {
    fn from(width_px: f32) -> Self {
        ImportedStroke {
            width_px,
            dash_px: None,
        }
    }
}

/// The most dashes one build cuts outlines into; past it, outlines are drawn solid. A dashed
/// county layer zoomed far in would otherwise cut every county's whole perimeter into dashes,
/// on and off screen alike.
const MAX_DASHES: usize = 200_000;

/// Theme-aware overlay build where each imported feature reads its own layer's outline
/// (ROADMAP_PARITY M4.1, M4.3): `imported[i]` for feature `i`, `None` leaving it out (its layer
/// is outside its zoom range), not drawing it transparent. Official products ignore it and keep
/// the established 1.6 px solid edge; an imported feature past the end of `imported` is left out.
pub fn build_layered(
    features: &[GeoFeature],
    zoom: f64,
    theme: crate::settings::Theme,
    imported: &[Option<ImportedStroke>],
) -> OverlayGeom {
    let mut geom = OverlayGeom::default();
    let mut fill_tess = FillTessellator::new();
    let mut stroke_tess = StrokeTessellator::new();
    let scale = crate::theme::overlay_stroke_scale(theme);
    let px = |width: f32| (width as f64 * scale as f64 / (256.0 * 2f64.powf(zoom))) as f32;
    // Fill tolerance is independent of the user-selected imported outline: a wide county border
    // should not make the polygon interior itself less accurate.
    let fill_opts = FillOptions::default().with_tolerance(px(1.6) * 0.5);
    let mut dash_budget = MAX_DASHES;

    for (i, f) in features.iter().enumerate() {
        let layer = imported.get(i).copied().flatten();
        // An imported layer outside its zoom range (I4) is left out, not drawn transparent.
        if f.kind == wxdata::overlay::FeatureKind::Imported && layer.is_none() {
            continue;
        }
        let width_px = feature_stroke_px(f.kind, layer.map_or(1.6, |l| l.width_px));
        let stroke_w = px(width_px);
        let mut stroke_opts = StrokeOptions::default()
            .with_line_width(stroke_w)
            .with_tolerance(stroke_w * 0.5);
        let path = feature_path(f);
        let dashed = layer
            .and_then(|l| l.dash_px)
            .filter(|_| f.kind == FeatureKind::Imported)
            .and_then(|(on, off)| {
                // A dot is a sliver of dash, its round caps making the disc.
                dash_path(f, px(on), px(off), &mut dash_budget).map(|p| (p, on <= 0.5))
            });
        let stroke_path = match dashed {
            Some((p, dots)) => {
                if dots {
                    stroke_opts = stroke_opts.with_line_cap(lyon::tessellation::LineCap::Round);
                }
                p
            }
            None => path.clone(),
        };
        let (fill_rgba, stroke_rgba) = high_contrast_feature_colors(f.fill, f.stroke, theme);
        let fill = color(fill_rgba);
        let stroke = color(stroke_rgba);

        let mut buf: VertexBuffers<OverlayVertex, u32> = VertexBuffers::new();
        let _ = fill_tess.tessellate_path(
            &path,
            &fill_opts,
            &mut BuffersBuilder::new(&mut buf, |v: FillVertex| OverlayVertex {
                offset: [0.0; 3],
                world: [v.position().x, v.position().y],
                color: fill,
            }),
        );
        let _ = stroke_tess.tessellate_path(
            &stroke_path,
            &stroke_opts,
            &mut BuffersBuilder::new(&mut buf, |v: StrokeVertex| OverlayVertex {
                offset: [0.0; 3],
                world: [v.position().x, v.position().y],
                color: stroke,
            }),
        );
        append(&mut geom, buf);
    }
    geom
}

/// Append tessellated placefile line/polygon geometry to `geom` (text/icons are drawn by the
/// egui painter). Items must be pre-filtered by threshold/time, and come paired with their
/// placefile's opacity (the Layer Manager's per-file dimmer). Line widths honor `zoom`.
pub fn append_placefiles(geom: &mut OverlayGeom, items: &[(&PlaceItem, f32)], zoom: f64) {
    append_placefiles_with_theme(geom, items, zoom, crate::settings::Theme::DearImGui)
}

/// Theme-aware variant: high-contrast doubles placefile line widths via `theme.rs`.
pub fn append_placefiles_with_theme(
    geom: &mut OverlayGeom,
    items: &[(&PlaceItem, f32)],
    zoom: f64,
    theme: crate::settings::Theme,
) {
    let mut fill_tess = FillTessellator::new();
    let mut stroke_tess = StrokeTessellator::new();
    let scale = crate::theme::overlay_stroke_scale(theme);
    let px = |w: f32| (w as f64 / (256.0 * 2f64.powf(zoom))) as f32;

    // World units per screen pixel at this zoom. `Object:` bodies are in pixels, and this is
    // what turns them into geometry — the overlay is retessellated when the zoom bucket changes,
    // so an anchored symbol keeps its size on screen instead of growing with the map.
    let world_per_px = 1.0 / (256.0 * 2f64.powf(zoom));
    for (item, opacity) in items {
        let color = |c: [u8; 4]| {
            let mut c = color(c);
            c[3] *= opacity;
            c
        };
        // Anchored (Object) coordinates are `[x_right, y_up]` pixel offsets; plain ones are
        // `[lon, lat]`. One closure so every statement below projects the same way.
        let project = |p: [f64; 2]| -> (f64, f64) {
            match item.anchor {
                Some([lon, lat]) => {
                    let (ax, ay) = lonlat_to_world(lon, lat);
                    (ax + p[0] * world_per_px, ay - p[1] * world_per_px)
                }
                None => lonlat_to_world(p[0], p[1]),
            }
        };
        match &item.kind {
            PlaceKind::Line {
                color: col,
                width,
                pts,
            } => {
                let stroke = color(*col);
                let mut b = Path::builder();
                let mut it = pts.iter().map(|&p| {
                    let (wx, wy) = project(p);
                    lyon::math::point(wx as f32, wy as f32)
                });
                if let Some(first) = it.next() {
                    b.begin(first);
                    for p in it {
                        b.line_to(p);
                    }
                    b.end(false);
                }
                let path = b.build();
                let opts = StrokeOptions::default()
                    .with_line_width(px(*width * scale).max(px(1.0 * scale)))
                    .with_line_cap(lyon::path::LineCap::Round)
                    .with_line_join(lyon::path::LineJoin::Round);
                let mut buf: VertexBuffers<OverlayVertex, u32> = VertexBuffers::new();
                let _ = stroke_tess.tessellate_path(
                    &path,
                    &opts,
                    &mut BuffersBuilder::new(&mut buf, |v: StrokeVertex| OverlayVertex {
                        offset: [0.0; 3],
                        world: [v.position().x, v.position().y],
                        color: stroke,
                    }),
                );
                append(geom, buf);
            }
            PlaceKind::Polygon { color: col, rings } => {
                let fill = color(*col);
                let mut b = Path::builder();
                for ring in rings {
                    let mut it = ring.iter().map(|&p| {
                        let (wx, wy) = project(p);
                        lyon::math::point(wx as f32, wy as f32)
                    });
                    if let Some(first) = it.next() {
                        b.begin(first);
                        for p in it {
                            b.line_to(p);
                        }
                        b.end(true);
                    }
                }
                let path = b.build();
                let opts = FillOptions::default();
                let mut buf: VertexBuffers<OverlayVertex, u32> = VertexBuffers::new();
                let _ = fill_tess.tessellate_path(
                    &path,
                    &opts,
                    &mut BuffersBuilder::new(&mut buf, |v: FillVertex| OverlayVertex {
                        offset: [0.0; 3],
                        world: [v.position().x, v.position().y],
                        color: fill,
                    }),
                );
                append(geom, buf);
            }
            PlaceKind::Triangles { verts } => {
                // No tessellator: the file already emitted triangles, so these go straight into
                // the buffers.
                let base = geom.vertices.len() as u32;
                for (p, c) in verts {
                    let (wx, wy) = project(*p);
                    geom.vertices.push(OverlayVertex {
                        offset: [0.0; 3],
                        world: [wx as f32, wy as f32],
                        color: color(*c),
                    });
                }
                geom.indices.extend(0..verts.len() as u32);
                geom.indices
                    .iter_mut()
                    .rev()
                    .take(verts.len())
                    .for_each(|i| *i += base);
            }
            PlaceKind::Image { verts, .. } => {
                // Fallback until the textured path lands: translucent white triangles so the
                // placement is visible and hit-testable. The full textured quad will sample the
                // image via its UVs; this path only proves the geometry survived the parse.
                let base = geom.vertices.len() as u32;
                let col = {
                    let mut c = color([255, 255, 255, 160]);
                    c[3] *= *opacity;
                    c
                };
                for (p, _uv) in verts {
                    let (wx, wy) = project(*p);
                    geom.vertices.push(OverlayVertex {
                        offset: [0.0; 3],
                        world: [wx as f32, wy as f32],
                        color: col,
                    });
                }
                geom.indices.extend(0..verts.len() as u32);
                geom.indices
                    .iter_mut()
                    .rev()
                    .take(verts.len())
                    .for_each(|i| *i += base);
            }
            PlaceKind::Text { .. } | PlaceKind::Icon { .. } => {} // painter pass
        }
    }
}

/// Build a closed lyon path (outer ring + holes) from a feature, in world coordinates.
fn feature_path(f: &GeoFeature) -> Path {
    let mut b = Path::builder();
    for ring in &f.rings {
        let mut pts = ring.iter().map(|&[lon, lat]| {
            let (wx, wy) = lonlat_to_world(lon, lat);
            lyon::math::point(wx as f32, wy as f32)
        });
        if let Some(first) = pts.next() {
            b.begin(first);
            for p in pts {
                b.line_to(p);
            }
            // A boundary's rings are open polylines; every other kind's are polygons.
            b.end(f.kind != FeatureKind::Boundary);
        }
    }
    b.build()
}

/// A feature's rings cut into dashes `on` long with `off` gaps, in world units, the pattern
/// running on round each ring including its closing edge. `None` (draw it solid) when that
/// would take more dashes than `budget` has left; the budget is spent otherwise.
fn dash_path(f: &GeoFeature, on: f32, off: f32, budget: &mut usize) -> Option<Path> {
    if !(on > 0.0 && off > 0.0) {
        return None;
    }
    let world = |&[lon, lat]: &[f64; 2]| {
        let (x, y) = lonlat_to_world(lon, lat);
        lyon::math::point(x as f32, y as f32)
    };
    let closed = f.kind != FeatureKind::Boundary;
    // Count first, so a feature over the budget costs nothing to refuse.
    let period = on + off;
    let mut needed = 0usize;
    for ring in &f.rings {
        let pts: Vec<_> = ring.iter().map(world).collect();
        let mut len: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if closed {
            if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                len += (*a - *b).length();
            }
        }
        needed += (len / period).ceil() as usize + 1;
        if needed > *budget {
            return None;
        }
    }
    *budget -= needed;
    let mut b = Path::builder();
    for ring in &f.rings {
        let mut pts: Vec<_> = ring.iter().map(world).collect();
        if closed && pts.len() > 2 && pts.first() != pts.last() {
            pts.push(pts[0]);
        }
        // Where along the pattern the walk is: [0, on) draws, [on, period) skips.
        let mut phase = 0.0f32;
        let mut drawing = false;
        for w in pts.windows(2) {
            let (a, d) = (w[0], w[1] - w[0]);
            let seg = d.length();
            if seg <= 0.0 {
                continue;
            }
            let mut t = 0.0f32;
            while t < seg {
                let left = if phase < on {
                    on - phase
                } else {
                    period - phase
                };
                let step = left.min(seg - t);
                // Past the precision of `t` the walk cannot advance; the rest of the edge is
                // left as it is rather than looping.
                if t + step <= t {
                    break;
                }
                let p0 = a + d * (t / seg);
                let p1 = a + d * ((t + step) / seg);
                if phase < on {
                    if !drawing {
                        b.begin(p0);
                        drawing = true;
                    }
                    b.line_to(p1);
                }
                t += step;
                phase += step;
                if phase >= on && drawing {
                    b.end(false);
                    drawing = false;
                }
                if phase >= period {
                    phase -= period;
                }
            }
        }
        if drawing {
            b.end(false);
        }
    }
    Some(b.build())
}

fn append(geom: &mut OverlayGeom, buf: VertexBuffers<OverlayVertex, u32>) {
    let base = geom.vertices.len() as u32;
    geom.vertices.extend(buf.vertices);
    geom.indices
        .extend(buf.indices.into_iter().map(|i| i + base));
}

fn high_contrast_feature_colors(
    fill: [u8; 4],
    stroke: [u8; 4],
    theme: crate::settings::Theme,
) -> ([u8; 4], [u8; 4]) {
    if crate::theme::is_high_contrast(theme) {
        let (fill_a, stroke_a) = crate::theme::warning_alpha_for(theme);
        // Keep the original RGB and scale the fill alpha by the same factor the warning fill gets
        // (45 -> 90, so 2x). Scaling rather than raising to a floor, because the fills are not all
        // meant to be equally visible: a watch box is stored at alpha 18 precisely so it reads as
        // a backdrop to the warnings drawn inside it. A floor of 90 erased that ordering and put
        // the watch at the same weight as the tornado warning on top of it.
        let scale =
            |a: u8, target: u8, from: u8| (a as u16 * target as u16 / from as u16).min(255) as u8;
        let boosted_fill = [fill[0], fill[1], fill[2], scale(fill[3], fill_a, 45)];
        // Outlines are the one thing high contrast does raise outright — an edge is either legible
        // or it is not, and every feature here already stores its stroke near-opaque.
        let boosted_stroke = [stroke[0], stroke[1], stroke[2], stroke_a];
        (boosted_fill, boosted_stroke)
    } else {
        (fill, stroke)
    }
}

#[cfg(test)]
mod high_contrast_tests {
    use super::*;
    use crate::settings::Theme;

    #[test]
    fn an_ordinary_theme_changes_nothing() {
        let f = [230, 40, 40, 45];
        let s = [230, 40, 40, 235];
        assert_eq!(high_contrast_feature_colors(f, s, Theme::DearImGui), (f, s));
    }

    #[test]
    fn custom_width_applies_only_to_imported_features_and_is_bounded() {
        assert_eq!(
            feature_stroke_px(wxdata::overlay::FeatureKind::Imported, 3.5),
            3.5
        );
        assert_eq!(
            feature_stroke_px(wxdata::overlay::FeatureKind::Imported, 99.0),
            8.0
        );
        assert_eq!(
            feature_stroke_px(wxdata::overlay::FeatureKind::Imported, 0.0),
            0.5
        );
        assert_eq!(
            feature_stroke_px(wxdata::overlay::FeatureKind::Warning, 7.0),
            1.6,
            "official warning geometry keeps its established outline"
        );
    }

    #[test]
    fn an_imported_shape_below_its_minimum_zoom_is_left_out() {
        let rings = vec![vec![
            [-98.0, 35.0],
            [-97.0, 35.0],
            [-97.0, 36.0],
            [-98.0, 35.0],
        ]];
        let (shapes, _) = crate::gis_import::to_renderable(vec![wxdata::gis::GisFeature {
            geometry: wxdata::gis::Geometry::Polygon(rings),
            properties: Default::default(),
        }]);
        let f = shapes[0].clone();
        assert_eq!(f.kind, wxdata::overlay::FeatureKind::Imported);
        let theme = crate::settings::Theme::DearImGui;
        let shown =
            build_with_theme_and_imported_width(std::slice::from_ref(&f), 6.0, theme, 1.6, true);
        let hidden = build_with_theme_and_imported_width(&[f], 6.0, theme, 1.6, false);
        assert!(!shown.indices.is_empty());
        assert!(hidden.indices.is_empty());
    }

    #[test]
    fn each_imported_feature_reads_its_own_layers_width_and_zoom() {
        let rings = vec![vec![
            [-98.0, 35.0],
            [-97.0, 35.0],
            [-97.0, 36.0],
            [-98.0, 35.0],
        ]];
        let (shapes, _) = crate::gis_import::to_renderable(vec![wxdata::gis::GisFeature {
            geometry: wxdata::gis::Geometry::Polygon(rings),
            properties: Default::default(),
        }]);
        let f = shapes[0].clone();
        let theme = crate::settings::Theme::DearImGui;
        let extent = |g: &OverlayGeom| {
            let xs = g.vertices.iter().map(|v| v.world[0]);
            xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min)
        };
        let thin = build_layered(std::slice::from_ref(&f), 6.0, theme, &[Some(1.0.into())]);
        let wide = build_layered(std::slice::from_ref(&f), 6.0, theme, &[Some(6.0.into())]);
        assert!(
            extent(&wide) > extent(&thin),
            "a wider layer outline reaches further"
        );
        // Two layers' copies of one shape: the hidden one is left out, the other drawn as its
        // own layer says, exactly as if alone.
        let both = build_layered(
            &[f.clone(), f.clone()],
            6.0,
            theme,
            &[Some(1.0.into()), None],
        );
        assert_eq!(both.indices.len(), thin.indices.len());
        assert_eq!(extent(&both), extent(&thin));
        // An official feature does not read the imported widths.
        let mut warning = f;
        warning.kind = wxdata::overlay::FeatureKind::Warning;
        let a = build_layered(std::slice::from_ref(&warning), 6.0, theme, &[None]);
        let b = build_layered(
            std::slice::from_ref(&warning),
            6.0,
            theme,
            &[Some(8.0.into())],
        );
        assert_eq!(extent(&a), extent(&b));
        assert!(!a.indices.is_empty());
    }

    /// The area the triangles of `g` cover, in world units squared.
    fn area(g: &OverlayGeom) -> f64 {
        g.indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| g.vertices[t[k] as usize].world);
                let (abx, aby) = (f64::from(b[0] - a[0]), f64::from(b[1] - a[1]));
                let (acx, acy) = (f64::from(c[0] - a[0]), f64::from(c[1] - a[1]));
                (abx * acy - aby * acx).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn a_dashed_outline_covers_its_share_of_the_solid_one() {
        let ring = vec![
            [-98.0, 35.0],
            [-97.0, 35.0],
            [-97.0, 36.0],
            [-98.0, 36.0],
            [-98.0, 35.0],
        ];
        let mut f = square(ring.clone());
        let theme = crate::settings::Theme::DearImGui;
        let stroke = |dash| ImportedStroke {
            width_px: 3.0,
            dash_px: dash,
        };
        // The fill's triangles cover the square itself; what is left is the outline.
        let world: Vec<(f64, f64)> = ring.iter().map(|p| lonlat_to_world(p[0], p[1])).collect();
        let square_area = world
            .windows(2)
            .map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1)
            .sum::<f64>()
            .abs()
            / 2.0;
        let one = std::slice::from_ref(&f);
        let outline =
            |dash| area(&build_layered(one, 8.0, theme, &[Some(stroke(dash))])) - square_area;
        let solid = outline(None);
        let (on, off) = crate::settings::LineDash::Dashed.pattern(3.0).unwrap();
        let share = outline(Some((on, off))) / solid;
        let want = f64::from(on / (on + off));
        assert!((share - want).abs() < 0.05, "{share} vs {want}");
        let dots = outline(crate::settings::LineDash::Dotted.pattern(3.0)) / solid;
        assert!(dots > 0.05 && dots < share, "{dots}");
        // An official feature is never dashed.
        f.kind = FeatureKind::Warning;
        let one = std::slice::from_ref(&f);
        let plain = build_layered(one, 8.0, theme, &[None]);
        let asked = build_layered(one, 8.0, theme, &[Some(stroke(Some((on, off))))]);
        assert_eq!(asked.indices.len(), plain.indices.len());
    }

    fn square(ring: Vec<[f64; 2]>) -> GeoFeature {
        GeoFeature {
            rings: vec![ring],
            fill: [80, 140, 220, 60],
            stroke: [80, 140, 220, 220],
            kind: FeatureKind::Imported,
            title: String::new(),
            detail: String::new(),
            alert: None,
        }
    }

    #[test]
    fn dashes_past_the_budget_fall_back_to_solid() {
        let f = square(vec![
            [-98.0, 35.0],
            [-97.0, 35.0],
            [-97.0, 36.0],
            [-98.0, 35.0],
        ]);
        let on = 1e-4;
        let mut budget = 10;
        assert!(dash_path(&f, on, on, &mut budget).is_none());
        assert_eq!(budget, 10, "a refusal spends nothing");
        let mut budget = MAX_DASHES;
        assert!(dash_path(&f, on, on, &mut budget).is_some());
        assert!(budget < MAX_DASHES);
    }
}
