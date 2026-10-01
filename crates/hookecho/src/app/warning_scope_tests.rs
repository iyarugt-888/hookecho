use super::{feature_in_box, GeoFeature};
use wxdata::overlay::FeatureKind;

fn poly(x0: f64, y0: f64, x1: f64, y1: f64) -> GeoFeature {
    GeoFeature {
        rings: vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]],
        fill: [0; 4],
        stroke: [0; 4],
        kind: FeatureKind::Warning,
        title: String::new(),
        detail: String::new(),
        alert: None,
    }
}

#[test]
fn feature_in_box_overlap() {
    // Box roughly around KFWS (Dallas): lon -97.3, lat 32.6, ±2.25°.
    let bx = (-99.55, 30.35, -95.05, 34.85);
    // A warning polygon overlapping the box.
    assert!(feature_in_box(&poly(-98.0, 32.0, -97.0, 33.0), bx));
    // A warning far away (Mississippi) — no overlap.
    assert!(!feature_in_box(&poly(-90.0, 32.0, -89.0, 33.0), bx));
    // Touching the edge counts as overlap.
    assert!(feature_in_box(&poly(-95.05, 32.0, -94.0, 33.0), bx));
    // Empty geometry never overlaps.
    let mut empty = poly(0.0, 0.0, 0.0, 0.0);
    empty.rings.clear();
    assert!(!feature_in_box(&empty, bx));
}
