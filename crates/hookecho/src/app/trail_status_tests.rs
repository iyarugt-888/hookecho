use super::trail_status_line;
use wxdata::extrema::{Extremum, Mismatch};

#[test]
fn says_how_far_along_a_trail_is_while_it_builds() {
    let line = trail_status_line(4, 12, 60, Extremum::Max, None);
    assert_eq!(line, "Building maximum trail: 4 of 12 cached volumes");
}

#[test]
fn names_the_kept_end_and_the_window_once_complete() {
    let line = trail_status_line(12, 12, 60, Extremum::Min, None);
    assert_eq!(line, "minimum of 12 cached volumes over 60 min");
}

#[test]
fn a_restart_always_says_why() {
    for (why, text) in [
        (Mismatch::Site, "the site changed"),
        (Mismatch::Elevation, "the tilt changed"),
        (Mismatch::Moment, "the product changed"),
        (Mismatch::Geometry, "the scan geometry changed"),
        (Mismatch::ValueRange, "raw and dealiased velocity differ"),
    ] {
        let line = trail_status_line(3, 3, 30, Extremum::Max, Some(why));
        assert!(line.ends_with(&format!("restarted: {text}")), "{line}");
    }
}
