//! ROADMAP_NEW C3's "radar coverage comparison between neighboring sites": which of two radars
//! has the lower (better) beam at each point, not just "which is nearest" — the suitability
//! popup (`ui::suitability_popup`) already answers that one point at a time via
//! `wxdata::suitability::rank`; this paints the same math as a map for a chosen pair.
//!
//! Pure geometry, same scope as `wxdata::suitability` itself: no network, no terrain, no live
//! data. What this deliberately does not attempt: folding in `elevation::blockage_fraction`'s
//! terrain occultation for either site — that needs a DEM tile fetch per site and is a genuinely
//! separate overlay a caller can already toggle alongside this one, not a reason to make this one
//! asynchronous.

use wxdata::sites::SiteEntry;
use wxdata::suitability::candidate_at;

/// Grid resolution of the painted raster (per side) — same as `elevation::RASTER_N`, so the two
/// overlays read at the same visual granularity.
pub const RASTER_N: usize = 256;

/// Beyond this beam height (m), neither radar is telling an analyst anything useful about
/// low-level structure — the comparison stops being operationally interesting past here, so the
/// overlay fades to nothing rather than painting a "winner" nobody could act on anyway.
const CEILING_M: f64 = 4_000.0;

/// Beam-height difference (m) inside which the two sites count as tied. Real numbers this close
/// are noise against every approximation the beam-height model itself already makes (4/3-earth,
/// no refraction anomalies, one WSR-88D-shaped beamwidth assumed for every candidate) — see
/// `wxdata::suitability`'s own doc comment on what it does not claim.
const DEADBAND_M: f64 = 150.0;

/// `site_a`'s advantage over `site_b` at `(lon, lat)`, in metres — positive means `site_a`'s beam
/// is lower (better) there. `None` where neither site's beam is within [`CEILING_M`], the same
/// "not operationally interesting" cutoff [`coverage_compare_image`] uses to leave a cell blank.
///
/// Antisymmetric by construction: swapping `site_a`/`site_b` negates the result (see this
/// module's own tests) — the map does not silently favor whichever site is passed first.
pub fn advantage_m(
    site_a: &'static SiteEntry,
    site_b: &'static SiteEntry,
    lon: f64,
    lat: f64,
    elevation_deg: f64,
) -> Option<f64> {
    let a = candidate_at(site_a, lon, lat, elevation_deg).beam_height_m;
    let b = candidate_at(site_b, lon, lat, elevation_deg).beam_height_m;
    if a.min(b) > CEILING_M {
        return None;
    }
    Some(b - a)
}

/// Diverging color for one pixel's `advantage_m`: transparent inside [`DEADBAND_M`], growing
/// opacity toward `color_a` (positive) or `color_b` (negative) as the margin widens, saturating
/// at [`CEILING_M`] worth of difference — the same deadband-plus-ramp shape
/// `fielddiff::diverging_lut` already uses for the model-comparison layers, so two different
/// "who's better here" overlays in this app read the same way.
fn advantage_color(advantage_m: f64, color_a: [u8; 3], color_b: [u8; 3]) -> egui::Color32 {
    if advantage_m.abs() <= DEADBAND_M {
        return egui::Color32::TRANSPARENT;
    }
    let span = (CEILING_M - DEADBAND_M).max(1.0);
    let mag = ((advantage_m.abs() - DEADBAND_M) / span).clamp(0.0, 1.0) as f32;
    let alpha = (60.0 + 150.0 * mag) as u8;
    let [r, g, b] = if advantage_m > 0.0 { color_a } else { color_b };
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// Build the coverage-comparison raster over world-space rect `[wx0, wy0, wx1, wy1]` — Mercator
/// world space, which maps linearly to screen, so the caller paints the result as one image with
/// no reprojection (same convention `elevation::blockage_image` uses). `site_a` paints blue where
/// its beam is the lower one, `site_b` paints red/orange where its own is.
pub fn coverage_compare_image(
    site_a: &'static SiteEntry,
    site_b: &'static SiteEntry,
    elevation_deg: f64,
    world: [f64; 4],
) -> egui::ColorImage {
    const COLOR_A: [u8; 3] = [70, 140, 230];
    const COLOR_B: [u8; 3] = [230, 90, 60];
    let mut px = vec![egui::Color32::TRANSPARENT; RASTER_N * RASTER_N];
    let (wx0, wy0, wx1, wy1) = (world[0], world[1], world[2], world[3]);
    for row in 0..RASTER_N {
        let wy = wy0 + (wy1 - wy0) * (row as f64 + 0.5) / RASTER_N as f64;
        for col in 0..RASTER_N {
            let wx = wx0 + (wx1 - wx0) * (col as f64 + 0.5) / RASTER_N as f64;
            let (lon, lat) = crate::render::mercator::world_to_lonlat(wx, wy);
            if let Some(adv) = advantage_m(site_a, site_b, lon, lat, elevation_deg) {
                px[row * RASTER_N + col] = advantage_color(adv, COLOR_A, COLOR_B);
            }
        }
    }
    egui::ColorImage {
        size: [RASTER_N, RASTER_N],
        pixels: px,
        source_size: egui::vec2(RASTER_N as f32, RASTER_N as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(id: &str) -> &'static SiteEntry {
        wxdata::sites::site_by_id(id).unwrap()
    }

    #[test]
    fn a_site_standing_on_its_own_antenna_beats_any_other_site() {
        let ktlx = site("KTLX");
        let kict = site("KICT"); // Wichita, a real neighbor a couple hundred km away
        let adv = advantage_m(ktlx, kict, ktlx.longitude as f64, ktlx.latitude as f64, 0.5)
            .expect("well within range of at least KTLX itself");
        assert!(adv > 0.0, "KTLX's own site should favor KTLX: {adv}");
    }

    #[test]
    fn swapping_the_pair_negates_the_advantage() {
        let ktlx = site("KTLX");
        let kict = site("KICT");
        // A point roughly between the two, not sitting on either antenna.
        let lon = (ktlx.longitude + kict.longitude) as f64 / 2.0;
        let lat = (ktlx.latitude + kict.latitude) as f64 / 2.0;
        let a_vs_b = advantage_m(ktlx, kict, lon, lat, 0.5);
        let b_vs_a = advantage_m(kict, ktlx, lon, lat, 0.5);
        match (a_vs_b, b_vs_a) {
            (Some(x), Some(y)) => assert!((x + y).abs() < 1e-6, "{x} vs {y}"),
            _ => panic!("midpoint of two real sites should be within range of both"),
        }
    }

    #[test]
    fn far_from_both_sites_is_not_operationally_interesting() {
        // The middle of the Pacific: neither a WSR-88D nor its neighbor has a usable beam there.
        let ktlx = site("KTLX");
        let kict = site("KICT");
        assert_eq!(advantage_m(ktlx, kict, -160.0, 10.0, 0.5), None);
    }

    #[test]
    fn color_is_transparent_inside_the_deadband_and_opaquer_further_out() {
        let color_a = [70, 140, 230];
        let color_b = [230, 90, 60];
        assert_eq!(
            advantage_color(0.0, color_a, color_b),
            egui::Color32::TRANSPARENT
        );
        assert_eq!(
            advantage_color(DEADBAND_M, color_a, color_b),
            egui::Color32::TRANSPARENT
        );
        let near = advantage_color(DEADBAND_M + 50.0, color_a, color_b);
        let far = advantage_color(CEILING_M, color_a, color_b);
        assert!(near.a() > 0 && far.a() > near.a(), "{near:?} vs {far:?}");
        // Sign picks the side: positive favors `color_a`, negative favors `color_b`.
        let positive = advantage_color(1000.0, color_a, color_b);
        let negative = advantage_color(-1000.0, color_a, color_b);
        assert!(positive.b() > positive.r(), "positive should read blue-ish");
        assert!(negative.r() > negative.b(), "negative should read red-ish");
    }

    #[test]
    fn the_raster_only_paints_inside_the_pixels_the_world_rect_covers() {
        let ktlx = site("KTLX");
        let kict = site("KICT");
        let half = 0.5; // world-space units, generously covering both sites' useful range
        let cx =
            crate::render::mercator::lonlat_to_world(ktlx.longitude as f64, ktlx.latitude as f64);
        let world = [cx.0 - half, cx.1 - half, cx.0 + half, cx.1 + half];
        let img = coverage_compare_image(ktlx, kict, 0.5, world);
        assert_eq!(img.size, [RASTER_N, RASTER_N]);
        let painted = img
            .pixels
            .iter()
            .filter(|p| **p != egui::Color32::TRANSPARENT)
            .count();
        assert!(painted > 0, "expected at least some painted cells");
        assert!(
            painted < img.pixels.len(),
            "expected some cells outside both sites' ceiling too"
        );
    }
}
