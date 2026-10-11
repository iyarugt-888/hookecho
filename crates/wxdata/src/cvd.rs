//! Colour-vision deficiency, simulated (1010.md V11): what a colour looks like with protanopia,
//! deuteranopia or tritanopia, and how far apart two colours are in CIELAB, so a test can say
//! whether a palette's or a product's colours stay distinct for someone who sees colour that way.
//!
//! The simulation is Machado, Oliveira and Fernandes (2009), "A physiologically-based model for
//! simulation of color vision deficiency", at full severity: a 3×3 matrix applied in linear RGB.
//! The distance is CIE76 ΔE in CIELAB (D65). Both are approximations: the model is of a typical
//! dichromat, and ΔE76 overstates differences between saturated colours. Good enough to tell a
//! pair that collapses from one that does not, not to certify a palette.

/// The three dichromacies the model covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deficiency {
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl Deficiency {
    pub const ALL: [Deficiency; 3] = [
        Deficiency::Protanopia,
        Deficiency::Deuteranopia,
        Deficiency::Tritanopia,
    ];

    fn matrix(self) -> [[f64; 3]; 3] {
        match self {
            Deficiency::Protanopia => [
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ],
            Deficiency::Deuteranopia => [
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ],
            Deficiency::Tritanopia => [
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_900],
            ],
        }
    }
}

fn to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(l: f64) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// `rgb` as someone with `d` would see it (sRGB in, sRGB out).
pub fn simulate(rgb: [u8; 3], d: Deficiency) -> [u8; 3] {
    let lin = rgb.map(to_linear);
    let m = d.matrix();
    let out: [f64; 3] =
        std::array::from_fn(|i| m[i][0] * lin[0] + m[i][1] * lin[1] + m[i][2] * lin[2]);
    out.map(to_srgb)
}

/// CIELAB (D65) of an sRGB colour.
pub fn lab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(to_linear);
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = (0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f64| {
        if t > 216.0 / 24_389.0 {
            t.cbrt()
        } else {
            (24_389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE76 ΔE between two sRGB colours: about 2.3 is a just-noticeable difference, and two
/// colours under about 10 apart are easily taken for each other at a glance.
pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (lab(a), lab(b));
    ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt()
}

/// The smallest ΔE between `a` and `b` across normal vision and the three dichromacies.
pub fn worst_delta_e(a: [u8; 3], b: [u8; 3]) -> f64 {
    Deficiency::ALL
        .iter()
        .map(|&d| delta_e(simulate(a, d), simulate(b, d)))
        .fold(delta_e(a, b), f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Greys stay grey under every simulation (each matrix's rows sum to one), and the Lab
    /// conversion puts white at L* 100 and black at 0.
    #[test]
    fn greys_are_unchanged_and_lab_has_its_ends() {
        for d in Deficiency::ALL {
            for v in [0u8, 64, 128, 200, 255] {
                let s = simulate([v, v, v], d);
                assert!(
                    s.iter().all(|c| (i16::from(*c) - i16::from(v)).abs() <= 1),
                    "{d:?} {v} {s:?}"
                );
            }
        }
        assert!((lab([255, 255, 255])[0] - 100.0).abs() < 0.01);
        assert!(lab([0, 0, 0])[0].abs() < 0.01);
        assert!(delta_e([10, 20, 30], [10, 20, 30]) < 1e-9);
    }

    /// The textbook confusion: pure red and pure green are far apart normally and close for a
    /// deuteranope or protanope, while blue and orange stay apart for all three.
    #[test]
    fn red_and_green_collapse_for_red_green_deficiency_but_blue_and_orange_do_not() {
        let (red, green) = ([255, 0, 0], [0, 200, 0]);
        let normal = delta_e(red, green);
        for d in [Deficiency::Protanopia, Deficiency::Deuteranopia] {
            let seen = delta_e(simulate(red, d), simulate(green, d));
            assert!(seen < normal * 0.4, "{d:?}: {seen:.1} vs {normal:.1}");
        }
        let (blue, orange) = ([33, 113, 181], [230, 120, 20]);
        assert!(worst_delta_e(blue, orange) > 40.0);
    }
}
