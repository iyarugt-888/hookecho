use super::{categorical_lut, distinct_tilts, glm_style, ramp_lut, ramp_lut_a, windy_url};

#[test]
fn distinct_tilts_skips_sails_repeats() {
    // A VCP 212 style list: 0.5 appears three times (SAILS), 0.9 twice (MRLE).
    let els = [0.5, 0.5, 0.9, 0.5, 0.9, 1.3, 1.8, 2.4];
    let picks = distinct_tilts(&els, 4);
    assert_eq!(
        picks,
        vec![0, 2, 5, 6],
        "one index per distinct angle, lowest first"
    );
    let angles: Vec<f32> = picks.iter().map(|&i| els[i]).collect();
    assert_eq!(angles, vec![0.5, 0.9, 1.3, 1.8]);
}

#[test]
fn distinct_tilts_clamps_to_what_exists() {
    assert_eq!(distinct_tilts(&[0.5, 0.5], 4), vec![0]);
    assert!(distinct_tilts(&[], 4).is_empty());
}

#[test]
fn windy_url_puts_latitude_first() {
    // KTLX, zoom 7.4. Windy wants lat,lon — this codebase says (lon, lat) everywhere else,
    // so a swap here would silently send people to the Indian Ocean.
    let u = windy_url("radar", -97.3, 35.4, 7.4);
    assert_eq!(u, "https://www.windy.com/?radar,35.400,-97.300,7");
    // Decimals are mandatory: Windy ignores a whole-number coordinate.
    assert!(windy_url("wind", -97.0, 35.0, 5.0).contains("35.000,-97.000"));
    // Zoom is clamped into Windy's own range rather than passed through.
    assert!(windy_url("wind", 0.0, 0.0, 2.0).ends_with(",3"));
    assert!(windy_url("wind", 0.0, 0.0, 18.9).ends_with(",18"));
}

#[test]
fn glm_flashes_fade_from_white_hot_to_ember() {
    let (fresh, r_fresh) = glm_style(0.0);
    let (old, r_old) = glm_style(900.0);
    assert_eq!(fresh.a(), 255, "a brand-new flash is fully opaque");
    assert!(old.a() < 100, "a 15-minute-old flash is nearly gone");
    assert!(r_fresh > r_old, "newest flashes draw largest");
    // Past the window the style clamps rather than inverting.
    assert_eq!(glm_style(5000.0), glm_style(900.0));
    // And it warms as it ages: green drops faster than red.
    assert!(old.g() < fresh.g() && old.r() <= fresh.r());
}

#[test]
fn categorical_lut_sets_only_listed_slots() {
    let lut = categorical_lut(&[(1, [10, 20, 30]), (7, [200, 40, 40])], 200);
    assert_eq!(lut.len(), 256 * 4);
    // Index 0 clear.
    assert_eq!(&lut[0..4], &[0, 0, 0, 0]);
    // Index 1 set with alpha 200.
    assert_eq!(&lut[4..8], &[10, 20, 30, 200]);
    // Index 7 set.
    assert_eq!(&lut[28..32], &[200, 40, 40, 200]);
    // An unlisted index stays clear.
    assert_eq!(&lut[8..12], &[0, 0, 0, 0]);
}

#[test]
fn ramp_lut_alpha_variants() {
    let opaque = ramp_lut(&[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])]);
    assert_eq!(opaque[255 * 4 + 3], 255, "top index opaque");
    assert_eq!(opaque[3], 0, "index 0 clear");
    let translucent = ramp_lut_a(&[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])], 150);
    assert_eq!(translucent[255 * 4 + 3], 150, "top index uses given alpha");
}
