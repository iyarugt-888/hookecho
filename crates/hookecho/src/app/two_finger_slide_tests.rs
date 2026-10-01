use super::split_two_finger_slide;

#[test]
fn a_flat_map_pans_with_the_whole_slide_and_never_tilts() {
    let (pan, pitch) = split_two_finger_slide(egui::vec2(3.0, -8.0), false);
    assert_eq!(pan, egui::vec2(3.0, -8.0));
    assert_eq!(pitch, 0.0);
}

#[test]
fn sliding_up_on_a_3d_map_raises_the_pitch_and_down_lowers_it() {
    let (_, up) = split_two_finger_slide(egui::vec2(0.0, -40.0), true);
    let (_, down) = split_two_finger_slide(egui::vec2(0.0, 40.0), true);
    assert_eq!(up, 10.0, "40 px up is 10 degrees more pitch");
    assert_eq!(down, -10.0);
}

#[test]
fn on_a_3d_map_the_vertical_part_no_longer_pans() {
    let (pan, _) = split_two_finger_slide(egui::vec2(6.0, -40.0), true);
    assert_eq!(
        pan,
        egui::vec2(6.0, 0.0),
        "horizontal still pans, vertical goes to the tilt"
    );
}

#[test]
fn no_movement_is_no_change() {
    assert_eq!(
        split_two_finger_slide(egui::Vec2::ZERO, true),
        (egui::Vec2::ZERO, 0.0)
    );
}
