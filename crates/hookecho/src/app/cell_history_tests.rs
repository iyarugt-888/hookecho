use super::*;
use chrono::TimeZone;

#[test]
fn history_prepends_tracked_cells_once_in_time_order() {
    let at = |m| Utc.with_ymd_and_hms(2026, 9, 27, 1, m, 0).unwrap();
    let live = ui::cell_window::CellSample {
        vil: Some(40.0),
        top: Some(30.0),
        dbz: Some(60.0),
        severity: Some(40),
        time: Some(at(20)),
        dbz_hgt: Some(19.0),
    };
    let mut trends = std::collections::HashMap::from([("B2".to_string(), vec![live])]);
    let cell = |id: &str, dbz| Cell {
        id: id.into(),
        max_dbz: Some(dbz),
        ..Default::default()
    };
    let past = vec![
        (at(10), vec![cell("B2", 52.0), cell("Z9", 50.0)]),
        (at(15), vec![cell("B2", 56.0)]),
        // The live scan again: not a second sample.
        (at(20), vec![cell("B2", 60.0)]),
    ];
    merge_cell_history(&mut trends, &past);
    let b2 = &trends["B2"];
    let dbz: Vec<_> = b2.iter().map(|s| s.dbz.unwrap()).collect();
    assert_eq!(dbz, vec![52.0, 56.0, 60.0]);
    assert!(b2[0].vil.is_none() && b2[2].vil.is_some());
    assert!(
        !trends.contains_key("Z9"),
        "a cell no longer tracked gets no trend"
    );
}
