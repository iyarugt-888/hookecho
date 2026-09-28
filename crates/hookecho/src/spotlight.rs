//! Alert spotlight: while an alert's card is open, the map outside its polygon is dimmed, so the
//! warned area is what the eye lands on (WeatherWise's "popup spotlight").
//!
//! The mask is the pane rectangle with the alert's rings cut out of it, tessellated even-odd so a
//! ring inside another (a hole in the alert) is dimmed again, and drawn as one mesh over the map.

use egui::{Color32, Mesh, Pos2, Rect};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};

/// How dark the dimmed map gets (alpha over black).
pub const DIM_ALPHA: u8 = 150;

/// The mesh that dims `rect` everywhere outside `rings` (screen points; each ring closed
/// implicitly). `None` when there is no ring with an area to cut out.
pub fn dim_outside(rect: Rect, rings: &[Vec<Pos2>], alpha: u8) -> Option<Mesh> {
    let rings: Vec<&Vec<Pos2>> = rings.iter().filter(|r| r.len() >= 3).collect();
    if rings.is_empty() {
        return None;
    }
    let mut b = Path::builder();
    // A little past the pane, so the mask's edge never shows as a seam at the pane border.
    let r = rect.expand(4.0);
    b.begin(point(r.left(), r.top()));
    b.line_to(point(r.right(), r.top()));
    b.line_to(point(r.right(), r.bottom()));
    b.line_to(point(r.left(), r.bottom()));
    b.close();
    for ring in rings {
        // Off-screen points are clamped far outside rather than dropped, so a ring the camera
        // only partly sees keeps its shape across the pane.
        let clamp = |p: &Pos2| {
            point(
                p.x.clamp(r.left() - 1.0e4, r.right() + 1.0e4),
                p.y.clamp(r.top() - 1.0e4, r.bottom() + 1.0e4),
            )
        };
        b.begin(clamp(&ring[0]));
        for p in &ring[1..] {
            b.line_to(clamp(p));
        }
        b.close();
    }
    let path = b.build();
    let color = Color32::from_black_alpha(alpha);
    let mut buf: VertexBuffers<Pos2, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default().with_fill_rule(FillRule::EvenOdd),
            &mut BuffersBuilder::new(&mut buf, |v: FillVertex| {
                let p = v.position();
                Pos2::new(p.x, p.y)
            }),
        )
        .ok()?;
    let mut mesh = Mesh::default();
    for p in buf.vertices {
        mesh.colored_vertex(p, color);
    }
    mesh.indices = buf.indices;
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `p` is inside any triangle of `mesh`.
    fn covered(mesh: &Mesh, p: Pos2) -> bool {
        mesh.indices.chunks(3).any(|t| {
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

    #[test]
    fn the_alert_stays_lit_and_everything_else_dims() {
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 300.0));
        let alert = vec![
            Pos2::new(100.0, 100.0),
            Pos2::new(200.0, 100.0),
            Pos2::new(200.0, 200.0),
            Pos2::new(100.0, 200.0),
        ];
        let mesh = dim_outside(rect, &[alert], DIM_ALPHA).unwrap();
        assert!(
            !covered(&mesh, Pos2::new(150.0, 150.0)),
            "inside the alert is lit"
        );
        assert!(covered(&mesh, Pos2::new(20.0, 20.0)), "outside is dimmed");
        assert!(covered(&mesh, Pos2::new(350.0, 250.0)));
        assert!(dim_outside(rect, &[], DIM_ALPHA).is_none());
    }
}
