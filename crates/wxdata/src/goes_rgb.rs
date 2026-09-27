//! RGB composites from GOES ABI channels (ROADMAP_NEW E4), as data rather than code.
//!
//! Every standard RGB is the same shape: three colour channels, each a band or a combination of
//! bands (usually a difference), stretched over a fixed range, sometimes gamma-curved, and read
//! against a published guide that says what each colour means. So a [`Recipe`] is exactly that
//! — three [`Channel`]s of weighted band terms, a range and a gamma — plus a line on what the
//! result shows. A blend product (the Sandwich: visible under a coloured, partly transparent IR
//! layer) is the same shape plus an [`IrOverlay`] laid over the stretched picture. One function
//! ([`compose`]) turns any recipe into an RGBA grid, and one
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

/// A coloured, partly transparent brightness-temperature layer laid over a recipe's picture:
/// the "sandwich" half of the Sandwich product. Pixels warmer than `warm` show the picture
/// alone; the overlay fades in down to `opaque_below`, where it reaches `alpha`. Colours come
/// from `ramp`, `(kelvin, rgb)` stops from warm to cold, interpolated linearly between stops.
/// The overlay colour is shaded by the picture's own brightness, so the visible texture (the
/// overshooting tops and gravity waves the product exists to show) stays readable through it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IrOverlay {
    pub band: u8,
    pub warm: f32,
    pub opaque_below: f32,
    pub alpha: f32,
    pub ramp: &'static [(f32, [u8; 3])],
}

impl IrOverlay {
    /// The overlay's colour and opacity for a brightness temperature; `None` where it shows
    /// nothing (warmer than `warm`, or no data).
    pub fn at(&self, k: f32) -> Option<([u8; 3], f32)> {
        if !k.is_finite() || k >= self.warm {
            return None;
        }
        let fade = ((self.warm - k) / (self.warm - self.opaque_below)).clamp(0.0, 1.0);
        let stops = self.ramp;
        let first = stops.first()?;
        let last = stops.last()?;
        let rgb = if k >= first.0 {
            first.1
        } else if k <= last.0 {
            last.1
        } else {
            let i = stops.windows(2).position(|w| k <= w[0].0 && k >= w[1].0)?;
            let ((k0, c0), (k1, c1)) = (stops[i], stops[i + 1]);
            let t = (k0 - k) / (k0 - k1);
            std::array::from_fn(|j| {
                (c0[j] as f32 + t * (c1[j] as f32 - c0[j] as f32)).round() as u8
            })
        };
        Some((rgb, fade * self.alpha))
    }

    /// Lay the overlay over one picture pixel for a brightness temperature `k`.
    pub fn blend(&self, pixel: [u8; 3], k: f32) -> [u8; 3] {
        let Some((rgb, a)) = self.at(k) else {
            return pixel;
        };
        // The picture's brightness shades the overlay colour: dark shadows stay darker, sunlit
        // tops brighter, so the texture shows through the colour.
        let lum =
            (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) / 255.0;
        let shade = 0.35 + 0.65 * lum;
        std::array::from_fn(|j| {
            let over = rgb[j] as f32 * shade;
            (pixel[j] as f32 * (1.0 - a) + over * a)
                .round()
                .clamp(0.0, 255.0) as u8
        })
    }
}

/// The night half of a day/night product: where the sun is down, an IR picture (cold cloud
/// bright, warm ground dark) instead of the recipe's reflective one, which has nothing to show
/// at night. The two are blended across the terminator by solar zenith angle, fully the day
/// picture at `day_zenith` degrees and below, fully the night one at `night_zenith` and above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NightBlend {
    pub band: u8,
    /// Brightness temperature (K) that reads as bare `ground`.
    pub warm: f32,
    /// Brightness temperature (K) that reads as full `cloud`.
    pub cold: f32,
    pub ground: [u8; 3],
    pub cloud: [u8; 3],
    pub day_zenith: f32,
    pub night_zenith: f32,
}

impl NightBlend {
    /// The night picture for a brightness temperature; `None` with no data.
    pub fn night(&self, k: f32) -> Option<[u8; 3]> {
        if !k.is_finite() {
            return None;
        }
        let t = ((self.warm - k) / (self.warm - self.cold)).clamp(0.0, 1.0);
        Some(std::array::from_fn(|j| {
            (self.ground[j] as f32 + t * (self.cloud[j] as f32 - self.ground[j] as f32)).round()
                as u8
        }))
    }

    /// How much of the day picture shows at a solar zenith angle (degrees): 1 in full sun, 0 at
    /// night, linear across the terminator.
    pub fn day_weight(&self, zenith: f32) -> f32 {
        ((self.night_zenith - zenith) / (self.night_zenith - self.day_zenith)).clamp(0.0, 1.0)
    }
}

/// The sun's declination (radians) and the equation of time (minutes) at `t` — NOAA's
/// low-precision series, good to a few tenths of a degree, far finer than a blend across a ten
/// degree terminator needs.
fn solar_terms(t: chrono::DateTime<chrono::Utc>) -> (f64, f64) {
    use chrono::{Datelike, Timelike};
    let hour = t.hour() as f64 + t.minute() as f64 / 60.0 + t.second() as f64 / 3600.0;
    let g = std::f64::consts::TAU / 365.0 * (t.ordinal() as f64 - 1.0 + (hour - 12.0) / 24.0);
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    (decl, eqtime)
}

/// The solar zenith angle (degrees) at a point, given [`solar_terms`] for the time.
fn solar_zenith(lat: f64, lon: f64, t: chrono::DateTime<chrono::Utc>, terms: (f64, f64)) -> f64 {
    use chrono::Timelike;
    let (decl, eqtime) = terms;
    let minutes = t.hour() as f64 * 60.0 + t.minute() as f64 + t.second() as f64 / 60.0;
    let true_solar = minutes + eqtime + 4.0 * lon;
    let hour_angle = (true_solar / 4.0 - 180.0).to_radians();
    let lat = lat.to_radians();
    let cos_z = lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos();
    cos_z.clamp(-1.0, 1.0).acos().to_degrees()
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
    /// A blend product's coloured IR layer over the stretched picture; `None` for a plain RGB.
    pub overlay: Option<IrOverlay>,
    /// A day/night product's IR night picture, blended in by solar zenith; `None` for a plain
    /// RGB, which shows its own channels whatever the sun is doing.
    pub night: Option<NightBlend>,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
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
    overlay: None,
    night: None,
};

/// The Sandwich: the half-kilometre red visible band, with the clean IR band's cold cloud tops
/// laid over it in colour, so the visible texture of a storm top (overshooting tops, above-anvil
/// plumes, gravity waves) and how cold it is read in one picture. The IR layer starts at -30 °C
/// and is fully tinted by -45 °C; the colour steps follow the usual CIRA enhancement, cyan
/// through green, yellow and red to magenta and white at the very coldest tops.
pub const SANDWICH: Recipe = Recipe {
    slug: "sandwich",
    name: "Sandwich (visible + IR)",
    reading: "Visible texture under colour-enhanced cold tops: blue-green around -40 °C, yellow               to red below -60 °C, magenta and white at the coldest overshooting tops.               Uncoloured cloud is warmer than -30 °C. Daylight only.",
    daytime: true,
    channels: [
        Channel::band(2, 0.0, 1.0, 1.4),
        Channel::band(2, 0.0, 1.0, 1.4),
        Channel::band(2, 0.0, 1.0, 1.4),
    ],
    overlay: Some(IrOverlay {
        band: 13,
        warm: 243.15,
        opaque_below: 228.15,
        alpha: 0.6,
        ramp: &[
            (243.15, [0, 200, 230]),
            (233.15, [0, 110, 255]),
            (223.15, [0, 210, 60]),
            (213.15, [255, 240, 0]),
            (203.15, [255, 110, 0]),
            (198.15, [220, 0, 0]),
            (193.15, [220, 0, 220]),
            (183.15, [255, 255, 255]),
        ],
    }),
    night: None,
};

/// Day/night colour, in the spirit of CIRA's GeoColor: true colour where the sun is up, and
/// where it is down the clean IR band as cloud over a dark blue ground, blended across the
/// terminator by solar zenith angle so a loop runs through dusk without going black. GeoColor
/// proper also lays city lights and a static surface under the night side and picks out low
/// cloud with the 3.9 µm difference; this has neither, so the night side shows the cloud the IR
/// band sees.
pub const DAY_NIGHT_COLOR: Recipe = Recipe {
    slug: "day-night-color",
    name: "Day/Night Color",
    reading: "By day, roughly what the eye would see. At night, cloud from the IR band: the               brighter, the colder and higher; dark blue is clear or warm low cloud. The two blend               through dusk and dawn.",
    daytime: false,
    channels: TRUE_COLOR.channels,
    overlay: None,
    night: Some(NightBlend {
        band: 13,
        warm: 295.0,
        cold: 215.0,
        ground: [8, 16, 44],
        cloud: [236, 240, 248],
        day_zenith: 80.0,
        night_zenith: 92.0,
    }),
};

/// Every recipe, in menu order.
pub const RECIPES: [Recipe; 9] = [
    AIR_MASS,
    DAY_CLOUD_PHASE,
    DAY_CONVECTION,
    DAY_NIGHT_COLOR,
    DUST,
    FIRE_TEMPERATURE,
    NIGHT_MICROPHYSICS,
    SANDWICH,
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
            .chain(self.overlay.map(|o| o.band))
            .chain(self.night.map(|n| n.band))
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
    for band in recipe.bands() {
        anyhow::ensure!(
            band_numbers.contains(&band),
            "{} needs band {band}",
            recipe.name
        );
    }
    let index = |band: u8| band_numbers.iter().position(|&b| b == band).unwrap_or(0);
    // Each channel's terms as (grid index, weight), resolved once rather than per pixel.
    let terms: Vec<Vec<(usize, f32)>> = recipe
        .channels
        .iter()
        .map(|c| c.terms.iter().map(|&(b, w)| (index(b), w)).collect())
        .collect();
    let overlay = recipe.overlay.map(|o| (o, index(o.band)));
    let night = recipe.night.map(|nb| (nb, index(nb.band)));
    let sun = solar_terms(first.time);
    let (nx, ny) = (first.nx, first.ny);
    let dlon = (first.lon_east - first.lon_west) / nx as f64;
    let dlat = (first.lat_north - first.lat_south) / ny as f64;
    let n = nx * ny;
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
        // A day/night product: the night picture where the sun is down, the day one where it is
        // up, mixed across the terminator. Either alone stands in where the other has no data.
        if let Some((nb, i)) = night {
            let (row, col) = (px / nx, px % nx);
            let lat = first.lat_north - (row as f64 + 0.5) * dlat;
            let lon = first.lon_west + (col as f64 + 0.5) * dlon;
            let w = nb.day_weight(solar_zenith(lat, lon, first.time, sun) as f32);
            match (valid && w > 0.0, nb.night(bands[i].values[px])) {
                (true, Some(dark)) => {
                    for j in 0..3 {
                        out[j] = (out[j] as f32 * w + dark[j] as f32 * (1.0 - w)).round() as u8;
                    }
                }
                (true, None) => {}
                (false, Some(dark)) => {
                    out[..3].copy_from_slice(&dark);
                    valid = true;
                }
                (false, None) => valid = false,
            }
        }
        if valid {
            // No IR under a visible pixel leaves the picture alone rather than blanking it.
            if let Some((o, i)) = overlay {
                let [r, g, b] = o.blend([out[0], out[1], out[2]], bands[i].values[px]);
                out[..3].copy_from_slice(&[r, g, b]);
            }
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

/// A composite as an ordinary scalar grid, so it can travel, cache and loop like every other
/// field: each cell's value is its colour packed into one number (`r * 65536 + g * 256 + b`,
/// exact in an `f32`, whose 24-bit mantissa holds every one of the 16.7 million colours), and a
/// cell with no data is `NaN`. [`unpack`] reads a value back.
pub fn pack(rgb: &RgbGrid) -> MrmsField {
    let values = rgb
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            if p[3] == 0 {
                f32::NAN
            } else {
                ((u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])) as f32
            }
        })
        .collect();
    MrmsField {
        values,
        nx: rgb.nx,
        ny: rgb.ny,
        lon_west: rgb.lon_west,
        lon_east: rgb.lon_east,
        lat_north: rgb.lat_north,
        lat_south: rgb.lat_south,
        time: rgb.time,
    }
}

/// The colour a [`pack`]ed value holds; `None` for no data (or a value that is not one).
pub fn unpack(v: f32) -> Option<[u8; 3]> {
    if !v.is_finite() || !(0.0..=16_777_215.0).contains(&v) {
        return None;
    }
    let c = v as u32;
    Some([(c >> 16) as u8, (c >> 8) as u8, c as u8])
}

/// A packed composite reduced to at most a given number of colours, for a renderer that draws an
/// indexed grid through a colour table: each cell's index into `palette` (`None` for no data).
#[derive(Debug, Clone, PartialEq)]
pub struct Indexed {
    pub palette: Vec<[u8; 3]>,
    pub index: Vec<Option<u8>>,
}

/// A colour's median-cut bin: 5 bits a channel, 32 768 bins.
fn bin_of(c: [u8; 3]) -> usize {
    (usize::from(c[0] >> 3) << 10) | (usize::from(c[1] >> 3) << 5) | usize::from(c[2] >> 3)
}

/// Channel `k` (0 red, 1 green, 2 blue) of a bin, 0..32.
fn bin_channel(bin: usize, k: usize) -> u8 {
    ((bin >> (10 - 5 * k)) & 31) as u8
}

/// A box's widest channel: how wide, and which.
fn widest(bins: &[usize]) -> (u8, usize) {
    let mut best = (0u8, 0usize);
    for k in 0..3 {
        let (lo, hi) = bins.iter().fold((31u8, 0u8), |(lo, hi), &b| {
            (lo.min(bin_channel(b, k)), hi.max(bin_channel(b, k)))
        });
        if hi - lo > best.0 {
            best = (hi - lo, k);
        }
    }
    best
}

/// Reduce a [`pack`]ed grid to at most `max_colors` (1..=256) colours by median cut: the colours
/// (at 5 bits a channel) are split, box by box, across the channel with the widest spread at the
/// pixel-weighted median, until there are `max_colors` boxes; each box's colour is its pixels'
/// mean. The box split next is the one whose spread times pixel count is largest, so a broad
/// family of colours gets more shades than a speck. An RGB composite is mostly a few broad colour
/// families (the guides read it that way), so 255 adaptive colours are close to
/// indistinguishable from the full image, where a fixed colour cube would band.
pub fn quantize(values: &[f32], max_colors: usize) -> Indexed {
    let max_colors = max_colors.clamp(1, 256);
    let mut count = vec![0u32; 1 << 15];
    let mut sum = vec![[0u64; 3]; 1 << 15];
    for c in values.iter().filter_map(|v| unpack(*v)) {
        let b = bin_of(c);
        count[b] += 1;
        for k in 0..3 {
            sum[b][k] += u64::from(c[k]);
        }
    }
    let used: Vec<usize> = (0..1usize << 15).filter(|&b| count[b] > 0).collect();
    let pixels = |bins: &[usize]| -> u64 { bins.iter().map(|&b| u64::from(count[b])).sum() };
    let mut boxes: Vec<Vec<usize>> = if used.is_empty() {
        Vec::new()
    } else {
        vec![used]
    };
    while boxes.len() < max_colors {
        let pick = boxes
            .iter()
            .enumerate()
            .filter(|(_, bx)| bx.len() > 1)
            .max_by_key(|(_, bx)| u64::from(widest(bx).0) * pixels(bx).max(1))
            .map(|(i, _)| i);
        let Some(i) = pick else { break };
        let mut bx = boxes.swap_remove(i);
        let k = widest(&bx).1;
        bx.sort_by_key(|&b| bin_channel(b, k));
        let total = pixels(&bx);
        let mut acc = 0u64;
        let mut cut = 1;
        for (j, &b) in bx.iter().enumerate() {
            acc += u64::from(count[b]);
            if acc * 2 >= total {
                cut = (j + 1).clamp(1, bx.len() - 1);
                break;
            }
        }
        let tail = bx.split_off(cut);
        boxes.push(bx);
        boxes.push(tail);
    }
    let mut palette = Vec::with_capacity(boxes.len());
    let mut of_bin = vec![0u8; 1 << 15];
    for (i, bx) in boxes.iter().enumerate() {
        let n = pixels(bx).max(1);
        let mut c = [0u64; 3];
        for &b in bx {
            for k in 0..3 {
                c[k] += sum[b][k];
            }
            of_bin[b] = i as u8;
        }
        palette.push([(c[0] / n) as u8, (c[1] / n) as u8, (c[2] / n) as u8]);
    }
    let index = values
        .iter()
        .map(|v| unpack(*v).map(|c| of_bin[bin_of(c)]))
        .collect();
    Indexed { palette, index }
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
    sector: crate::goes_abi::Sector,
    at: Option<chrono::DateTime<chrono::Utc>>,
    recipe: &Recipe,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<RgbGrid> {
    let bands = recipe.bands();
    // A mesoscale sector scans every minute; its bands of one scan are seconds apart.
    let same = if sector.is_meso() { 30 } else { SAME_SCAN_SECS };
    let fields = crate::goes_abi::fetch_same_scan(
        client, satellite, sector, at, &bands, same, out_nx, out_ny,
    )
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
            // Reflective bands only answer in daylight, and the flag says so, unless a night
            // picture takes over after dark.
            let reflective = r.bands().iter().any(|b| *b <= 6);
            assert_eq!(r.daytime, reflective && r.night.is_none(), "{}", r.name);
        }
        assert_eq!(AIR_MASS.bands(), [8, 10, 12, 13]);
        assert_eq!(TRUE_COLOR.bands(), [1, 2, 3]);
        assert_eq!(
            DAY_NIGHT_COLOR.bands(),
            [1, 2, 3, 13],
            "and the night picture's"
        );
        assert_eq!(
            SANDWICH.bands(),
            [2, 13],
            "the overlay's band is fetched too"
        );
        for r in RECIPES {
            let Some(o) = r.overlay else { continue };
            assert!(o.warm > o.opaque_below && o.alpha > 0.0 && o.alpha <= 1.0);
            assert!(
                o.ramp.windows(2).all(|w| w[0].0 > w[1].0),
                "{}: ramp stops run warm to cold",
                r.name
            );
        }
    }

    #[test]
    fn the_sun_is_overhead_at_the_subsolar_point_and_down_at_its_antipode() {
        use chrono::TimeZone;
        // The June solstice at noon over Greenwich: the sun stands over the Tropic of Cancer.
        let t = chrono::Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let terms = solar_terms(t);
        assert!(solar_zenith(23.44, 0.0, t, terms) < 1.0);
        assert!(solar_zenith(-23.44, 180.0, t, terms) > 179.0);
        // Oklahoma at 20 UTC in late September is mid-afternoon; at 06 UTC it is night.
        let afternoon = chrono::Utc.with_ymd_and_hms(2026, 9, 27, 20, 0, 0).unwrap();
        let z = solar_zenith(35.3, -97.3, afternoon, solar_terms(afternoon));
        assert!((40.0..60.0).contains(&z), "{z}");
        let night = chrono::Utc.with_ymd_and_hms(2026, 9, 27, 6, 0, 0).unwrap();
        assert!(solar_zenith(35.3, -97.3, night, solar_terms(night)) > 100.0);
    }

    #[test]
    fn day_night_color_is_true_color_by_day_and_ir_by_night() {
        use chrono::TimeZone;
        // One pixel over Oklahoma: bands 1, 2, 3 then 13. Bright reflective cloud, cold top.
        let at = |t, refl: f32, k: f32| {
            let g = |v: f32| {
                let mut f = grid(vec![v]);
                (f.lon_west, f.lon_east, f.lat_north, f.lat_south) = (-97.5, -97.0, 35.5, 35.0);
                f.time = t;
                f
            };
            let b = [g(refl), g(refl), g(refl), g(k)];
            compose(&DAY_NIGHT_COLOR, &[1, 2, 3, 13], &b).unwrap().rgba
        };
        let day = chrono::Utc.with_ymd_and_hms(2026, 9, 27, 19, 0, 0).unwrap();
        let night = chrono::Utc.with_ymd_and_hms(2026, 9, 27, 7, 0, 0).unwrap();
        // By day, what the reflective bands say, whatever the IR says.
        assert_eq!(
            at(day, 0.0, 220.0)[..3],
            [0, 0, 0],
            "a black surface stays black by day"
        );
        // By night, the IR: a cold top bright, warm ground dark blue; reflectance ignored.
        let cold = at(night, 0.0, 215.0);
        assert!(cold[0] > 200 && cold[3] == 255, "{cold:?}");
        let warm = at(night, 0.9, 300.0);
        assert_eq!(warm[..3], DAY_NIGHT_COLOR.night.unwrap().ground);
        // At night a missing reflective band does not blank the IR picture.
        let dark = at(night, f32::NAN, 215.0);
        assert_eq!(dark[3], 255);
        // And by day a missing IR band leaves the true colour.
        assert_eq!(at(day, 0.5, f32::NAN)[3], 255);
    }

    #[test]
    fn the_sandwich_colours_only_cold_tops_and_keeps_the_visible_texture() {
        // Band 2 then band 13 for four pixels: warm bright cloud, a -50 °C anvil, a -85 °C
        // overshooting top, and a sunlit anvil with no IR.
        let vis = grid(vec![0.8, 0.8, 0.8, 0.5]);
        let ir = grid(vec![270.0, 223.15, 188.15, f32::NAN]);
        let rgb = compose(&SANDWICH, &[2, 13], &[vis.clone(), ir.clone()]).unwrap();
        let px = |i: usize| &rgb.rgba[i * 4..i * 4 + 4];
        let warm = px(0);
        assert!(
            warm[0] == warm[1] && warm[1] == warm[2],
            "warm cloud stays grey: {warm:?}"
        );
        let anvil = px(1);
        assert!(
            anvil[1] > anvil[0] && anvil[1] > anvil[2],
            "-50 °C reads green: {anvil:?}"
        );
        let top = px(2);
        assert!(
            top[0] > 200 && top[2] > 200,
            "-85 °C reads magenta-white: {top:?}"
        );
        let no_ir = px(3);
        assert_eq!(no_ir[3], 255, "missing IR keeps the visible picture");
        assert!(no_ir[0] == no_ir[1] && no_ir[1] == no_ir[2]);
        // A darker visible pixel under the same cold top comes out darker: texture survives.
        let dim = grid(vec![0.2, 0.2, 0.2, 0.2]);
        let shaded = compose(&SANDWICH, &[2, 13], &[dim, ir]).unwrap();
        let sum = |p: &[u8]| p[..3].iter().map(|&c| c as u32).sum::<u32>();
        assert!(sum(&shaded.rgba[4..8]) < sum(&rgb.rgba[4..8]));
        // No visible band: nothing to lay the IR over, so no picture.
        let dark = compose(&SANDWICH, &[2, 13], &[grid(vec![f32::NAN; 4]), vis]).unwrap();
        assert_eq!(dark.rgba[3], 0);
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

    #[test]
    fn a_composite_packs_into_a_field_and_back_exactly() {
        let rgb = RgbGrid {
            rgba: vec![255, 0, 128, 255, 1, 2, 3, 0, 0, 0, 0, 255],
            nx: 3,
            ny: 1,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 40.0,
            lat_south: 30.0,
            time: chrono::DateTime::UNIX_EPOCH,
        };
        let f = pack(&rgb);
        assert_eq!(unpack(f.values[0]), Some([255, 0, 128]));
        assert!(f.values[1].is_nan(), "alpha 0 is no data");
        assert_eq!(
            unpack(f.values[2]),
            Some([0, 0, 0]),
            "black is a colour, not no data"
        );
        assert_eq!(unpack(16_777_215.0), Some([255, 255, 255]));
        assert_eq!(unpack(f32::NAN), None);
        assert_eq!(unpack(-1.0), None);
        assert_eq!((f.nx, f.ny, f.lon_west), (3, 1, -100.0));
    }

    fn packed(c: [u8; 3]) -> f32 {
        ((u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])) as f32
    }

    #[test]
    fn quantizing_keeps_a_few_colours_exact_and_many_close() {
        // Three colours, fewer than the budget: each gets its own entry, exactly.
        let few = [
            packed([200, 0, 0]),
            packed([0, 200, 0]),
            packed([0, 0, 200]),
            f32::NAN,
            packed([200, 0, 0]),
        ];
        let q = quantize(&few, 255);
        assert_eq!(q.palette.len(), 3);
        assert_eq!(q.index[3], None);
        assert_eq!(q.index[0], q.index[4]);
        assert_eq!(q.palette[q.index[0].unwrap() as usize], [200, 0, 0]);
        // A smooth ramp of 4096 colours into 16: every pixel lands close to its entry.
        let ramp: Vec<f32> = (0..4096u32)
            .map(|i| packed([(i % 64 * 4) as u8, (i / 64 * 4) as u8, 100]))
            .collect();
        let q = quantize(&ramp, 16);
        assert!(q.palette.len() <= 16);
        let worst = ramp
            .iter()
            .zip(&q.index)
            .map(|(val, i)| {
                let c = unpack(*val).unwrap();
                let p = q.palette[i.unwrap() as usize];
                (0..3)
                    .map(|k| (i32::from(c[k]) - i32::from(p[k])).abs())
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert!(worst <= 40, "worst channel error {worst}");
        // With the full budget the same ramp is near exact.
        let q = quantize(&ramp, 255);
        assert!(q.palette.len() > 200);
        // Nothing to show: an empty palette and every cell empty.
        let none = quantize(&[f32::NAN, f32::NAN], 255);
        assert!(none.palette.is_empty() && none.index.iter().all(Option::is_none));
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_and_packs_a_real_air_mass_composite() {
        let client = reqwest::Client::new();
        let rgb = fetch_recipe(
            &client,
            Satellite::East,
            crate::goes_abi::Sector::Conus,
            None,
            &AIR_MASS,
            600,
            350,
        )
        .await
        .unwrap();
        let field = pack(&rgb);
        let valid = field.values.iter().filter(|v| v.is_finite()).count();
        let q = quantize(&field.values, 254);
        eprintln!(
            "air mass {} x {} at {}: {:.0}% covered, {} palette colours",
            rgb.nx,
            rgb.ny,
            rgb.time,
            100.0 * valid as f64 / field.values.len() as f64,
            q.palette.len()
        );
        assert!(valid * 2 > field.values.len(), "most of CONUS is covered");
        assert!(q.palette.len() > 100, "a real scene has many colours");
    }

    /// A daytime recipe, with band 2's ~70 MB half-kilometre granule in it. Run in daylight over
    /// the U.S.; at night it still composes, just dark.
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_a_real_day_cloud_phase_composite() {
        let client = reqwest::Client::new();
        let rgb = fetch_recipe(
            &client,
            Satellite::East,
            crate::goes_abi::Sector::Conus,
            None,
            &DAY_CLOUD_PHASE,
            600,
            350,
        )
        .await
        .unwrap();
        let q = quantize(&pack(&rgb).values, 254);
        eprintln!(
            "day cloud phase at {}: {} colours",
            rgb.time,
            q.palette.len()
        );
        assert!(q.palette.len() > 50);
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_a_real_sandwich_over_a_mesoscale_sector() {
        let client = reqwest::Client::new();
        let rgb = fetch_recipe(
            &client,
            Satellite::East,
            crate::goes_abi::Sector::Meso1,
            None,
            &SANDWICH,
            480,
            480,
        )
        .await
        .unwrap();
        let px: Vec<&[u8]> = rgb.rgba.chunks(4).filter(|p| p[3] == 255).collect();
        let tinted = px
            .iter()
            .filter(|p| !(p[0] == p[1] && p[1] == p[2]))
            .count();
        eprintln!(
            "sandwich at {}: {} pixels, {:.1}% tinted by cold tops",
            rgb.time,
            px.len(),
            100.0 * tinted as f64 / px.len().max(1) as f64
        );
        assert!(px.len() * 2 > rgb.nx * rgb.ny, "most of the box is covered");
    }
}
