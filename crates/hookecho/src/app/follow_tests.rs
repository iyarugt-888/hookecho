use super::{nearest_cell, widget_storm_line};
use wxdata::level3::Cell;

fn cell(id: &str, lon: f64, lat: f64) -> Cell {
    Cell {
        id: id.into(),
        lon,
        lat,
        ..Default::default()
    }
}

#[test]
fn the_widget_line_reads_from_where_you_are() {
    // A cell due west of the reader, moving northeast at 30 kt.
    let mut c = cell("R3", -97.72, 35.30);
    c.mvt_deg = Some(45.0);
    c.mvt_kt = Some(30.0);
    let line = widget_storm_line(&[c.clone()], -97.5, 35.3, false).unwrap();
    // West of you: the compass point names the side the storm is on, not the bearing to it.
    assert!(line.starts_with("R3 12 mi W"), "{line}");
    assert!(line.ends_with("moving NE 35 mph"), "{line}");
    // Same cell on a German pane: the distance turns over, the speed does not.
    let km_line = widget_storm_line(&[c], -97.5, 35.3, true).unwrap();
    assert!(km_line.starts_with("R3 20 km W"), "{km_line}");
    // Nothing within 300 km is no line at all.
    assert!(widget_storm_line(&[cell("Z1", -80.0, 35.3)], -97.5, 35.3, false).is_none());
    assert!(widget_storm_line(&[], -97.5, 35.3, false).is_none());
}

#[test]
fn nearest_cell_within_radius_and_none_outside() {
    // Three cells around a predicted point near (−97.5, 35.3).
    let cells = vec![
        cell("A7", -97.60, 35.30), // ~9 km west
        cell("B3", -97.51, 35.31), // ~1.5 km — the nearest
        cell("", -97.505, 35.305), // closest of all but no SCIT id → ineligible
    ];
    let got = nearest_cell(&cells, -97.5, 35.3, 15.0).unwrap();
    assert_eq!(got.id, "B3");
    // A prediction far from every cell (radius exceeded) → nothing to adopt.
    assert!(nearest_cell(&cells, -90.0, 30.0, 15.0).is_none());
}
