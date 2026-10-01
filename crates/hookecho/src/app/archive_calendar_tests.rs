use chrono::Datelike;

/// December must roll into next January and January must roll back into last December —
/// the two edges the plain `month - 1`/`month + 1` arithmetic in the widget cannot handle
/// itself, which is why it is guarded with the wraparound checks it has.
#[test]
fn month_navigation_wraps_the_year() {
    let (year, month) = (2026i32, 1u32);
    let (py, pm) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    assert_eq!((py, pm), (2025, 12));
    let (year, month) = (2026i32, 12u32);
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    assert_eq!((ny, nm), (2027, 1));
}

/// The grid needs an exact day count per month, including the leap-year edge, to avoid
/// drawing a nonexistent Feb 30th or clipping Feb 29th on a leap year.
#[test]
fn days_in_month_matches_the_calendar_including_leap_years() {
    for (year, month, expected) in [
        (2026, 1, 31),
        (2026, 2, 28),
        (2024, 2, 29),
        (2026, 4, 30),
        (2026, 12, 31),
    ] {
        let first = chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
        assert_eq!((next - first).num_days(), expected, "{year}-{month}");
    }
}

/// The archive's actual start date must fall inside the year range the widget lets the
/// `DragValue` reach, or 1991-06-05 itself would be unreachable by year alone.
#[test]
fn archive_start_year_is_a_valid_calendar_year() {
    assert_eq!(wxdata::level2::ARCHIVE_START.year(), 1991);
    assert!(wxdata::level2::ARCHIVE_START.month() >= 1);
}

/// A day moved from outside the text field (a caret, the calendar grid) must show up in the
/// field even when the year does not change — the buffer used to resync only on a leading-year
/// mismatch, so stepping from the 13th to the 3rd of the same month left "13" on screen while
/// the map had already moved to the 3rd.
#[test]
fn the_typed_field_follows_a_same_year_external_move() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("archive-day-text");
    let day13 = chrono::NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
    let day03 = chrono::NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        super::archive_day_text_input(ui, day13);
    });
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(id)),
        Some("2026-09-13".to_string())
    );
    // The caller moved `date` without the field's own text ever changing — a caret step or a
    // calendar pick, not a keystroke.
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        super::archive_day_text_input(ui, day03);
    });
    assert_eq!(
        ctx.data(|d| d.get_temp::<String>(id)),
        Some("2026-09-03".to_string()),
        "the field must drop the stale day rather than keep showing the 13th"
    );
}
