//! RGB composites from GOES ABI channels (ROADMAP_NEW E4), as data rather than code.
//!
//! Every standard RGB is the same shape: three colour channels, each a band or a combination of
//! bands (usually a difference), stretched over a fixed range, sometimes gamma-curved, and read
//! against a published guide that says what each colour means. So a [`Recipe`] is exactly that
//! — three [`Channel`]s of weighted band terms, a range and a gamma — plus a line on what the
//! result shows. One function ([`compose`]) turns any recipe into an RGBA grid, and one
//! ([`fetch_recipe`]) fetches its bands from a single scan. Adding a recipe is adding a constant.
//!
//! The ranges and gammas are the operational ones from the CIRA/RAMMB GOES-R RGB quick guides
//! (the EUMETSAT conventions adapted to ABI bands). A range written high-to-low inverts the
//! channel, as the guides do for "colder is brighter". Gamma follows the same convention: the
//! stretched value `t` becomes `t^(1/gamma)`.
//!
//! Reflective bands (1-6) arrive as reflectance factor 0..1, emissive bands (7-16) as brightness
//! temperature in kelvin — what [`crate::goes_abi::decode`] returns for each.

use crate::goes_abi::Satellite;
use crate::mrms::MrmsField;

/// One colour channel: `sum(weight * band)`, stretched from `lo` (0) to `hi` (255), then
/// gamma-curved. `lo > hi` inverts the channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Channel {
    pub terms: &'static [(u8, f32)],
    pub lo: f32,
    pub hi: f32,
    pub gamma: f32,
}

impl Channel {
    const fn band(band: u8, lo: f32, hi: f32, gamma: f32) -> Channel {
        Channel {
            terms: match band {
                1 => &[(1, 1.0)],
                2 => &[(2, 1.0)],
                5 => &[(5, 1.0)],
                6 => &[(6, 1.0)],
                7 => &[(7, 1.0)],
                8 => &[(8, 1.0)],
                13 => &[(13, 1.0)],
                _ => &[],
            },
            lo,
            hi,
            gamma,
        }
    }

    /// The stretched, gamma-curved 0..=255 value for a pixel whose channel sum is `v`.
    pub fn stretch(&self, v: f32) -> u8 {
        let t = ((v - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0);
        let t = if self.gamma == 1.0 {
            t
        } else {
            t.powf(1.0 / self.gamma)
        };
        (t * 255.0).round() as u8
    }
}

/// A named RGB composite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    /// Stable in settings and links.
    pub slug: &'static str,
    pub name: &'static str,
    /// What the colours mean, in a line — the guide's own reading, shortened.
    pub reading: &'static str,
    /// Only meaningful in daylight: a reflective band carries it.
    pub daytime: bool,
    pub channels: [Channel; 3],
}

/// Air mass: jet streaks, dry intrusions and potential-vorticity anomalies. Red is the 6.2-7.3 µm
/// water-vapour difference, green the 9.6-10.3 µm ozone difference, blue the inverted 6.2 µm.
pub const AIR_MASS: Recipe = Recipe {
    slug: "air-mass",
    name: "Air Mass",
    reading: "Red-orange: dry, ozone-rich stratospheric air (possible jet/PV anomaly). Green: \
              warm, moist tropical air. Blue-purple: cold polar air. White: thick high cloud.",
    daytime: false,
    channels: [
        Channel {
            terms: &[(8, 1.0), (10, -1.0)],
            lo: -26.2,
            hi: 0.6,
            gamma: 1.0,
        },
        Channel {
            terms: &[(12, 1.0), (13, -1.0)],
            lo: -43.2,
            hi: 6.7,
            gamma: 1.0,
        },
        Channel::band(8, 243.9, 208.5, 1.0),
    ],
};

/// Dust: airborne dust (magenta) against cloud and surface, day or night.
pub const DUST: Recipe = Recipe {
    slug: "dust",
    name: "Dust",
    reading: "Magenta/pink: dust. Dark red: thick high ice cloud. Light blue-grey: low water \
              cloud. Blue-violet: clear desert surface.",
    daytime: false,
    channels: [
        Channel {
            terms: &[(15, 1.0), (13, -1.0)],
            lo: -6.7,
            hi: 2.6,
            gamma: 1.0,
        },
        Channel {
            terms: &[(13, 1.0), (11, -1.0)],
            lo: -0.5,
            hi: 20.0,
            gamma: 2.5,
        },
        Channel::band(13, 261.2, 288.7, 1.0),
    ],
};

/// Night microphysics: fog and low stratus versus clear ground at night.
pub const NIGHT_MICROPHYSICS: Recipe = Recipe {
    slug: "night-microphysics",
    name: "Nighttime Microphysics",
    reading: "Aqua/pale green: fog and low water cloud. Red/dark red: thick cold ice cloud. \
              Pink/magenta-grey: clear ground. Only meaningful at night.",
    daytime: false,
    channels: [
        Channel {
            terms: &[(15, 1.0), (13, -1.0)],
            lo: -6.7,
            hi: 2.6,
            gamma: 1.0,
        },
        Channel {
            terms: &[(13, 1.0), (7, -1.0)],
            lo: -3.1,
            hi: 5.2,
            gamma: 1.0,
        },
        Channel::band(13, 243.55, 292.65, 1.0),
    ],
};

/// Day cloud phase: glaciating tops (developing convection) against water cloud and snow.
pub const DAY_CLOUD_PHASE: Recipe = Recipe {
    slug: "day-cloud-phase",
    name: "Day Cloud Phase Distinction",
    reading: "Yellow/orange: glaciating cloud tops (convection maturing). Green-cyan: low water \
              cloud. Red/pink: thick ice cloud. Blue-green: snow on the ground.",
    daytime: true,
    channels: [
        Channel::band(13, 280.65, 219.65, 1.0),
        Channel::band(2, 0.0, 0.78, 1.0),
        Channel::band(5, 0.01, 0.59, 1.0),
    ],
};

/// Day convection: strong updrafts with small ice particles (yellow) in severe convection.
pub const DAY_CONVECTION: Recipe = Recipe {
    slug: "day-convection",
    name: "Day Convection",
    reading: "Bright yellow: strong updrafts with small ice (intense convection). Orange-red: \
              ordinary deep convection. Blue/green: low cloud and surface.",
    daytime: true,
    channels: [
        Channel {
            terms: &[(8, 1.0), (10, -1.0)],
            lo: -35.0,
            hi: 5.0,
            gamma: 1.0,
        },
        Channel {
            terms: &[(7, 1.0), (13, -1.0)],
            lo: -5.0,
            hi: 60.0,
            gamma: 1.0,
        },
        Channel {
            terms: &[(5, 1.0), (2, -1.0)],
            lo: -0.75,
            hi: 0.25,
            gamma: 1.0,
        },
    ],
};

/// Fire temperature: active fires from warm (red) to intense (yellow-white).
pub const FIRE_TEMPERATURE: Recipe = Recipe {
    slug: "fire-temperature",
    name: "Fire Temperature",
    reading: "Red: a warm or small fire. Orange to yellow: hotter, larger fires. Near white: very \
              intense fire. Cloud and ground stay dark or blue-green.",
    daytime: true,
    channels: [
        Channel::band(7, 273.0, 333.0, 0.4),
        Channel::band(6, 0.0, 1.0, 1.0),
        Channel::band(5, 0.0, 0.75, 1.0),
    ],
};

/// True colour (daytime): red and blue are ABI's red and blue, and the green ABI lacks is
/// synthesized from red, blue and the 0.86 µm "veggie" band, the standard CIRA recipe.
pub const TRUE_COLOR: Recipe = Recipe {
    slug: "true-color",
    name: "True Color (day)",
    reading: "Roughly what the eye would see: white cloud, green vegetation, brown desert, blue \
              water. Daylight only; the synthetic green is an estimate.",
    daytime: true,
    channels: [
        Channel::band(2, 0.0, 1.0, 2.2),
        Channel {
            terms: &[(2, 0.45), (3, 0.10), (1, 0.45)],
            lo: 0.0,
            hi: 1.0,
            gamma: 2.2,
        },
        Channel::band(1, 0.0, 1.0, 2.2),
    ],
};

/// Every recipe, in menu order.
pub const RECIPES: [Recipe; 7] = [
    AIR_MASS,
    DAY_CLOUD_PHASE,
    DAY_CONVECTION,
    DUST,
    FIRE_TEMPERATURE,
    NIGHT_MICROPHYSICS,
    TRUE_COLOR,
];

/// A recipe by its slug.
pub fn by_slug(slug: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.slug == slug)
}

impl Recipe {
    /// Every band the recipe reads, ascending, each once.
    pub fn bands(&self) -> Vec<u8> {
        let mut b: Vec<u8> = self
            .channels
            .iter()
            .flat_map(|c| c.terms.iter().map(|&(band, _)| band))
            .collect();
        b.sort_unstable();
        b.dedup();
        b
    }
}

/// An RGBA composite on the same lat/lon grid its bands were decoded to.
#[derive(Debug, Clone, PartialEq)]
pub struct RgbGrid {
    /// Row-major `ny × nx` RGBA; alpha 0 where any band has no data.
    pub rgba: Vec<u8>,
    pub nx: usize,
    pub ny: usize,
    pub lon_west: f64,
    pub lon_east: f64,
    pub lat_north: f64,
    pub lat_south: f64,
    /// The scan time of the recipe's first band.
    pub time: chrono::DateTime<chrono::Utc>,
}

/// Compose `recipe` from its bands' grids (`bands[i]` is band `band_numbers[i]`). All grids must
/// be the same shape — true for any bands decoded at the same output size.
pub fn compose(
    recipe: &Recipe,
    band_numbers: &[u8],
    bands: &[MrmsField],
) -> anyhow::Result<RgbGrid> {
    anyhow::ensure!(
        band_numbers.len() == bands.len() && !bands.is_empty(),
        "one grid per band"
    );
    let first = &bands[0];
    anyhow::ensure!(
        bands.iter().all(|b| b.nx == first.nx && b.ny == first.ny),
        "band grids have different shapes"
    );
    for c in &recipe.channels {
        for (band, _) in c.terms {
            anyhow::ensure!(
                band_numbers.contains(band),
                "{} needs band {band}",
                recipe.name
            );
        }
    }
    let index = |band: u8| band_numbers.iter().position(|&b| b == band).unwrap_or(0);
    // Each channel's terms as (grid index, weight), resolved once rather than per pixel.
    let terms: Vec<Vec<(usize, f32)>> = recipe
        .channels
        .iter()
        .map(|c| c.terms.iter().map(|&(b, w)| (index(b), w)).collect())
        .collect();
    let n = first.nx * first.ny;
    let mut rgba = vec![0u8; n * 4];
    for px in 0..n {
        let mut out = [0u8; 4];
        let mut valid = true;
        for (ch, channel) in recipe.channels.iter().enumerate() {
            let mut v = 0.0f32;
            for &(i, w) in &terms[ch] {
                let x = bands[i].values[px];
                if !x.is_finite() {
                    valid = false;
                }
                v += w * x;
            }
            out[ch] = channel.stretch(v);
        }
        if valid {
            out[3] = 255;
            rgba[px * 4..px * 4 + 4].copy_from_slice(&out);
        }
    }
    Ok(RgbGrid {
        rgba,
        nx: first.nx,
        ny: first.ny,
        lon_west: first.lon_west,
        lon_east: first.lon_east,
        lat_north: first.lat_north,
        lat_south: first.lat_south,
        time: first.time,
    })
}

/// How far apart two bands' scan times may be and still count as one scan. A CONUS scan covers
/// every band within about a minute; a full five-minute gap means a different scan.
pub const SAME_SCAN_SECS: i64 = 120;

/// Fetch every band `recipe` needs from the newest scan on `satellite` — the newest key of its
/// first band, then each other band's key nearest that time, refused if it is a different scan —
/// decoded to `out_nx × out_ny`, and compose them.
pub async fn fetch_recipe(
    client: &reqwest::Client,
    satellite: Satellite,
    recipe: &Recipe,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<RgbGrid> {
    let bands = recipe.bands();
    let fields =
        crate::goes_abi::fetch_same_scan(client, satellite, &bands, SAME_SCAN_SECS, out_nx, out_ny)
            .await?;
    compose(recipe, &bands, &fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(values: Vec<f32>) -> MrmsField {
        MrmsField {
            nx: values.len(),
            ny: 1,
            values,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 40.0,
            lat_south: 30.0,
            time: chrono::DateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_channel_stretches_clamps_inverts_and_curves() {
        let c = Channel::band(13, 200.0, 300.0, 1.0);
        assert_eq!(c.stretch(200.0), 0);
        assert_eq!(c.stretch(250.0), 128);
        assert_eq!(c.stretch(300.0), 255);
        assert_eq!(c.stretch(400.0), 255, "clamped");
        let inv = Channel::band(13, 300.0, 200.0, 1.0);
        assert_eq!(inv.stretch(200.0), 255, "a high-to-low range inverts");
        // Gamma 2.2 lifts the dark end: a quarter of the way up reads well above a quarter.
        let g = Channel::band(2, 0.0, 1.0, 2.2);
        assert!(g.stretch(0.25) > 128, "{}", g.stretch(0.25));
        assert_eq!(g.stretch(0.0), 0);
    }

    #[test]
    fn every_recipe_names_real_bands_and_a_nonzero_range() {
        for r in RECIPES {
            assert!(!r.bands().is_empty(), "{}", r.name);
            for c in r.channels {
                assert!(!c.terms.is_empty(), "{}: a channel with no terms", r.name);
                assert!(c.lo != c.hi && c.gamma > 0.0, "{}", r.name);
                for (band, _) in c.terms {
                    assert!((1..=16).contains(band), "{}: band {band}", r.name);
                }
            }
            assert_eq!(by_slug(r.slug), Some(&r));
            // Reflective bands only answer in daylight, and the flag says so.
            assert_eq!(r.daytime, r.bands().iter().any(|b| *b <= 6), "{}", r.name);
        }
        assert_eq!(AIR_MASS.bands(), [8, 10, 12, 13]);
        assert_eq!(TRUE_COLOR.bands(), [1, 2, 3]);
    }

    #[test]
    fn air_mass_colours_follow_the_guide() {
        // Bands 8, 10, 12, 13 for two pixels: a dry stratospheric intrusion (large 6.2-7.3
        // difference toward 0, warm 6.2 µm → red channel high, blue low) and a moist tropical one.
        let b8 = grid(vec![240.0, 225.0]);
        let b10 = grid(vec![240.0, 250.0]);
        let b12 = grid(vec![250.0, 250.0]);
        let b13 = grid(vec![280.0, 290.0]);
        let rgb = compose(&AIR_MASS, &[8, 10, 12, 13], &[b8, b10, b12, b13]).unwrap();
        let (dry, moist) = (&rgb.rgba[0..4], &rgb.rgba[4..8]);
        assert!(dry[0] > 200 && dry[2] < 40, "dry air reads red: {dry:?}");
        assert!(moist[0] < dry[0], "moist air is less red: {moist:?}");
        assert_eq!((dry[3], moist[3]), (255, 255));
    }

    #[test]
    fn a_pixel_missing_any_band_is_transparent_and_shapes_must_agree() {
        let a = grid(vec![250.0, f32::NAN]);
        let rgb = compose(&DUST, &[11, 13, 15], &[a.clone(), a.clone(), a.clone()]).unwrap();
        assert_eq!(rgb.rgba[3], 255);
        assert_eq!(rgb.rgba[7], 0, "no data anywhere in the sum: transparent");
        assert!(
            compose(&DUST, &[11, 13], &[a.clone(), a.clone()]).is_err(),
            "band 15 missing"
        );
        let short = grid(vec![250.0]);
        assert!(compose(&DUST, &[11, 13, 15], &[a.clone(), a, short]).is_err());
    }
}
