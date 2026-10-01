//! Convex envelopes for the swept uncertainty of each storm-line segment.

pub(super) fn hull(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.retain(|p| p.iter().all(|v| v.is_finite()));
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for p in &points {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    for p in points.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

pub(super) fn touches(ring: &[[f64; 2]], zone: &[[f64; 2]]) -> bool {
    match ring {
        [] => false,
        [p] => wxdata::overlay::point_in_ring(zone, p[0], p[1]),
        [a, b] => wxdata::overlay::segment_intersects_ring(*a, *b, zone),
        _ => wxdata::overlay::rings_intersect(ring, zone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_handles_duplicate_interior_collinear_and_nonfinite_points() {
        let square = hull(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.5, 0.5],
            [0.0, 0.0],
            [f64::NAN, 0.0],
        ]);
        assert_eq!(square, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(
            hull(vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]),
            [[0.0, 0.0], [2.0, 0.0]]
        );
    }
}
