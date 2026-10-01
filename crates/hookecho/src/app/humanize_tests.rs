use super::humanize;

#[test]
fn ages_stay_readable_all_the_way_back_to_the_archive() {
    assert_eq!(humanize(45), "45s");
    assert_eq!(humanize(600), "10m");
    assert_eq!(humanize(3 * 3600 + 20 * 60), "3h20m");
    // Past a couple of days, hours stop meaning anything to a reader.
    assert_eq!(humanize(5 * 86_400), "5d");
    // Moore 2013, seen from 2026 — used to render as "113000h40m ago".
    assert_eq!(humanize(13 * 365 * 86_400), "13.0y");
}
