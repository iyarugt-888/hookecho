use super::nowcast_confidence;

#[test]
fn confidence_is_full_inside_the_old_range_and_fades_past_it() {
    for lead in [15u8, 30, 45] {
        assert_eq!(nowcast_confidence(lead), 1.0, "{lead} min");
    }
    assert!(nowcast_confidence(60) < 1.0);
    assert!(nowcast_confidence(90) < nowcast_confidence(60));
    assert!((nowcast_confidence(120) - 0.35).abs() < 1e-5);
}

/// It must never fade to invisible, or the layer would silently stop existing.
#[test]
fn confidence_never_reaches_zero() {
    for lead in 0u8..=255 {
        assert!(nowcast_confidence(lead) >= 0.35, "{lead} min");
    }
}
