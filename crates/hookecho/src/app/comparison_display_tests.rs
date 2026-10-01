use super::{field_draw_opacity, format_diff_readout, workstation_too_narrow};

#[test]
fn the_workstation_steps_aside_at_phone_width_only() {
    assert!(workstation_too_narrow(390.0));
    assert!(!workstation_too_narrow(768.0), "a tablet keeps it");
    assert!(
        !workstation_too_narrow(f32::INFINITY),
        "before the first frame"
    );
}
use crate::fielddiff::DiffMode;
use crate::render::FieldLayer as FL;

#[test]
fn only_the_upper_comparison_side_is_dimmed_and_user_opacity_is_preserved() {
    assert_eq!(field_draw_opacity(FL::CompareA, 0.8, true), 0.8);
    assert_eq!(field_draw_opacity(FL::CompareB, 0.8, true), 0.4);
    assert_eq!(field_draw_opacity(FL::CompareB, 0.8, false), 0.8);
    assert_eq!(field_draw_opacity(FL::Mrms, 0.6, true), 0.6);
}

#[test]
fn mask_readout_names_the_class_and_keeps_the_underlying_magnitude() {
    assert_eq!(
        format_diff_readout(DiffMode::Disagreement, 0.5, 1.0, "°C"),
        "Agree · Δ 0.5 °C"
    );
    assert_eq!(
        format_diff_readout(DiffMode::Disagreement, -2.5, 1.0, "°C"),
        "Disagree · Δ 2.5 °C"
    );
}
