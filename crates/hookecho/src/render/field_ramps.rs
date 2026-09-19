//! One table describing how every gridded field layer is colored — and what its colors mean.
//!
//! Before this, each layer's value→index mapping and color stops lived inline in a `match` arm in
//! `app.rs`, which made a legend impossible: nothing outside that function knew a layer's range or
//! units. The table is now the single source for BOTH the LUT the GPU samples and the legend the
//! user reads, so a scale can't drift from its own key.

use super::FieldLayer;

/// How raw grid values map onto the 2..=255 index range the LUT is baked over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampScale {
    /// `(v - lo) / (hi - lo)`.
    Linear,
    /// Linear on `|v|` — signed fields (azimuthal shear) whose sign is direction, not magnitude.
    Abs,
    /// `log10` between `lo` and `hi`; rainfall and recurrence intervals span decades.
    Log,
}

/// A continuous color scale, or a set of labeled category slots.
pub enum FieldScale {
    Ramp {
        lo: f32,
        hi: f32,
        scale: RampScale,
        /// `(t, rgb)` with `t` in 0..=1, interpolated.
        stops: &'static [(f32, [u8; 3])],
    },
    /// `(raw grid value, rgb, label)` — drawn as discrete swatches, no interpolation.
    Categorical(&'static [(u8, [u8; 3], &'static str)]),
}

/// How one field layer is colored and labeled.
pub struct FieldRamp {
    /// Legend heading.
    pub label: &'static str,
    /// Physical units, empty when the values are categories or already unitless.
    pub units: &'static str,
    pub scale: FieldScale,
    /// Opacity for non-zero indices; environment overlays sit translucent over the basemap.
    pub alpha: u8,
    /// Multiplier applied to raw grid values before anything else, when the wire units aren't the
    /// units anyone reads (HRRR smoke arrives in kg/m³; people talk in µg/m³).
    pub input_scale: f32,
    /// This ramp's `lo`/`hi`/`units` are Kelvin — a GRIB wire unit nobody reads. The legend
    /// converts to [`crate::settings::TempUnit`] for display; the GPU LUT stays in Kelvin
    /// (`index()` never sees this flag, so the color mapping is untouched).
    pub is_temp_kelvin: bool,
}

impl FieldRamp {
    /// Raw grid value in this layer's display units (applies [`Self::input_scale`]).
    pub fn display(&self, v: f32) -> f32 {
        v * self.input_scale
    }

    /// The `(lo, hi, units)` the legend should print for this ramp's endpoints. Every ramp but
    /// the two Kelvin ones just hands its own numbers back; those two convert through
    /// `unit.from_c` (an offset, not a scale, so [`Self::input_scale`] cannot express it) and
    /// swap in the unit's own label. The GPU color mapping in [`Self::index`] never calls this —
    /// it stays in Kelvin regardless of what the reader sees.
    pub fn legend_bounds(&self, unit: crate::settings::TempUnit) -> (f32, f32, &'static str) {
        let FieldScale::Ramp { lo, hi, .. } = &self.scale else {
            return (0.0, 0.0, self.units);
        };
        if self.is_temp_kelvin {
            (
                unit.from_c(*lo - 273.15),
                unit.from_c(*hi - 273.15),
                unit.label(),
            )
        } else {
            (*lo, *hi, self.units)
        }
    }

    /// Raw grid value → LUT index. Index 0 is "nothing here" (fully transparent).
    pub fn index(&self, v: f32) -> u8 {
        let v = self.display(v);
        match &self.scale {
            FieldScale::Categorical(_) => v as u8,
            FieldScale::Ramp {
                lo,
                hi,
                scale,
                stops: _,
            } => {
                let v = if *scale == RampScale::Abs { v.abs() } else { v };
                if v < *lo {
                    return 0;
                }
                let t = match scale {
                    RampScale::Log => (v.log10() - lo.log10()) / (hi.log10() - lo.log10()),
                    _ => (v - lo) / (hi - lo),
                };
                (2.0 + t.clamp(0.0, 1.0) * 253.0) as u8
            }
        }
    }
}

macro_rules! ramp {
    ($label:expr, $units:expr, $lo:expr, $hi:expr, $scale:expr, $alpha:expr, $stops:expr) => {
        FieldRamp {
            label: $label,
            units: $units,
            alpha: $alpha,
            input_scale: 1.0,
            is_temp_kelvin: false,
            scale: FieldScale::Ramp {
                lo: $lo,
                hi: $hi,
                scale: $scale,
                stops: $stops,
            },
        }
    };
}

static ROTATION: FieldRamp = ramp!(
    "Rotation",
    "\u{d7}10\u{207b}\u{b3} s\u{207b}\u{b9}",
    4.0,
    40.0,
    RampScale::Abs,
    255,
    &[
        (0.0, [40, 90, 200]),
        (0.4, [40, 200, 200]),
        (0.7, [240, 230, 60]),
        (1.0, [230, 40, 40]),
    ]
);

/// Wind speed, for the animated particle layer.
///
/// Public and deliberately absent from [`ramp_for`]: wind is not a [`FieldLayer`] — nothing is
/// uploaded to the GPU as a scalar grid — but the particles still need a value→color rule and the
/// legend still needs to explain it. `input_scale` converts the model's m/s to the knots people
/// actually read, so both get the same conversion for free.
pub static WIND: FieldRamp = FieldRamp {
    input_scale: 1.943_844,
    ..ramp!(
        "Wind",
        "kt",
        0.0,
        // 50, not 80: surface wind is 5-20 kt almost everywhere almost always, and a scale topping
        // out at jet speeds leaves every particle in the pale bottom tenth of the palette. The
        // live decode test peaked at 39 kt over the whole of CONUS.
        50.0,
        RampScale::Linear,
        255,
        &[
            (0.0, [120, 165, 205]),
            (0.25, [95, 200, 175]),
            (0.5, [235, 225, 120]),
            (0.7, [240, 155, 70]),
            (0.85, [225, 80, 70]),
            (1.0, [230, 95, 200]),
        ]
    )
};

static MESH: FieldRamp = ramp!(
    "Hail size",
    "mm",
    10.0,
    75.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [60, 200, 90]),
        (0.4, [240, 230, 60]),
        (0.7, [240, 150, 30]),
        (1.0, [230, 60, 200]),
    ]
);

static HAIL_SWATH: FieldRamp = ramp!(
    "Hail swaths (24 h)",
    "mm",
    19.0,
    75.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [60, 200, 90]),
        (0.4, [240, 230, 60]),
        (0.7, [240, 150, 30]),
        (1.0, [230, 60, 200]),
    ]
);

static QPE_STOPS: &[(f32, [u8; 3])] = &[
    (0.0, [40, 180, 90]),
    (0.3, [230, 220, 60]),
    (0.55, [230, 110, 40]),
    (0.8, [220, 40, 60]),
    (1.0, [230, 220, 240]),
];
// Rate, not accumulation: the top of the scale is a rain rate rather than a storm total, so
// it shares the QPE colours but not its bounds. 100 mm/hr is a tropical downpour.
static PRECIP_RATE: FieldRamp = ramp!(
    "Rain rate",
    "mm/hr",
    0.25,
    100.0,
    RampScale::Log,
    255,
    QPE_STOPS
);
static QPE_1H: FieldRamp = ramp!(
    "Rain (1 h)",
    "mm",
    0.25,
    100.0,
    RampScale::Log,
    255,
    QPE_STOPS
);
static QPE_24H: FieldRamp = ramp!(
    "Rain (24 h)",
    "mm",
    0.25,
    250.0,
    RampScale::Log,
    255,
    QPE_STOPS
);
// Shared by the 3/6/12 h windows: a working middle ground between the 1 h and 24 h scales.
static QPE_MID: FieldRamp = ramp!(
    "Rain (3-12 h)",
    "mm",
    0.25,
    150.0,
    RampScale::Log,
    255,
    QPE_STOPS
);

static CAPE: FieldRamp = ramp!(
    "CAPE",
    "J/kg",
    100.0,
    5000.0,
    RampScale::Linear,
    150,
    &[
        (0.0, [0, 200, 200]),
        (0.25, [40, 200, 90]),
        (0.5, [240, 230, 60]),
        (0.75, [240, 150, 30]),
        (1.0, [230, 60, 200]),
    ]
);

static SRH: FieldRamp = ramp!(
    "Helicity",
    "m\u{b2}/s\u{b2}",
    50.0,
    500.0,
    RampScale::Linear,
    150,
    &[
        (0.0, [40, 90, 200]),
        (0.5, [240, 230, 60]),
        (1.0, [230, 40, 40]),
    ]
);

static FLASH_FLOOD: FieldRamp = ramp!(
    "Flood recurrence",
    "yr",
    1.0,
    100.0,
    RampScale::Log,
    255,
    &[
        (0.0, [240, 230, 60]),
        (0.3, [240, 150, 30]),
        (0.6, [230, 40, 40]),
        (0.85, [150, 40, 200]),
        (1.0, [240, 240, 240]),
    ]
);

static VIL: FieldRamp = ramp!(
    "Water aloft (VIL)",
    "kg/m\u{b2}",
    0.1,
    80.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [60, 200, 90]),
        (0.35, [240, 230, 60]),
        (0.6, [240, 150, 30]),
        (0.85, [230, 60, 200]),
        (1.0, [240, 240, 240]),
    ]
);

static ECHO_TOPS: FieldRamp = ramp!(
    "Storm tops",
    "kft",
    5.0,
    70.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [40, 90, 200]),
        (0.4, [40, 200, 90]),
        (0.75, [240, 230, 60]),
        (1.0, [240, 240, 240]),
    ]
);

/// VIL density: water aloft per unit storm depth. Above ~3.5 g/m³ is the classic large-hail
/// signature, so the scale turns hot exactly there rather than spending its range on drizzle.
static VIL_DENSITY: FieldRamp = ramp!(
    "VIL density",
    "g/m\u{b3}",
    0.5,
    5.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [40, 90, 200]),
        (0.45, [60, 200, 90]),
        (0.65, [240, 230, 60]),
        (0.8, [240, 150, 30]),
        (1.0, [230, 60, 200]),
    ]
);

/// GLM flash-extent density: flashes per cell over the last 15 minutes. One flash is worth
/// showing (it is the first one), and a vigorous updraft runs into the tens, so the scale is
/// short and warms fast.
static GLM_FED: FieldRamp = ramp!(
    "Flash density",
    "flashes/15 min",
    1.0,
    30.0,
    RampScale::Linear,
    220,
    &[
        (0.0, [60, 60, 160]),
        (0.35, [90, 180, 230]),
        (0.6, [240, 230, 60]),
        (0.8, [240, 140, 40]),
        (1.0, [255, 60, 60]),
    ]
);

/// Probability of severe hail. 50% is Witt's warning threshold, so the scale turns warm there.
static POSH: FieldRamp = ramp!(
    "Severe hail probability",
    "%",
    10.0,
    100.0,
    RampScale::Linear,
    255,
    &[
        (0.0, [40, 90, 200]),
        (0.45, [240, 230, 60]),
        (0.75, [240, 150, 30]),
        (1.0, [230, 60, 200]),
    ]
);

/// Global-model mean sea-level pressure. The band is wide because this is a whole-planet field:
/// a deep low and a summer ridge have to share one scale.
static GLOBAL_MSLP: FieldRamp = FieldRamp {
    input_scale: 0.01, // Pa → hPa
    ..ramp!(
        "MSLP",
        "hPa",
        960.0,
        1040.0,
        RampScale::Linear,
        170,
        &[
            (0.0, [150, 60, 200]),
            (0.35, [60, 120, 220]),
            (0.5, [230, 230, 230]),
            (0.7, [240, 170, 60]),
            (1.0, [200, 60, 60]),
        ]
    )
};

/// 500 hPa geopotential height — the steering flow, in decametres the way charts label it.
static GLOBAL_HEIGHT_500: FieldRamp = FieldRamp {
    input_scale: 0.1, // m → dam
    ..ramp!(
        "500 hPa height",
        "dam",
        492.0,
        600.0,
        RampScale::Linear,
        170,
        &[
            (0.0, [120, 60, 190]),
            (0.3, [60, 110, 220]),
            (0.55, [90, 200, 160]),
            (0.8, [240, 200, 70]),
            (1.0, [220, 70, 50]),
        ]
    )
};

/// Global 2 m temperature, in the units most of the planet reads.
static GLOBAL_DEWPOINT_2M: FieldRamp = FieldRamp {
    // Kelvin like the temperature ramp, but a moisture scale rather than a thermal one: brown
    // and dry at the bottom, green and soupy at the top, with the 15 °C / 288 K "muggy" mark
    // near the middle where severe forecasters look.
    input_scale: 1.0,
    is_temp_kelvin: true,
    ..ramp!(
        "2 m dewpoint",
        "K",
        243.0,
        300.0,
        RampScale::Linear,
        170,
        &[
            (0.0, [120, 90, 60]),
            (0.35, [210, 200, 170]),
            (0.6, [110, 190, 130]),
            (0.8, [30, 140, 90]),
            (1.0, [20, 80, 110]),
        ]
    )
};

/// Simplified enhanced-IR curve: grayscale (cold = bright, warm = dark, the traditional IR sense)
/// through ordinary cloud tops, with a colorized pop for the coldest overshooting convection —
/// the same idea as the classic NHC/McIDAS enhancement curves, without their full non-monotonic
/// repeating bands.
static GOES_IR: FieldRamp = FieldRamp {
    input_scale: 1.0,
    is_temp_kelvin: true,
    ..ramp!(
        "IR brightness temp",
        "K",
        180.0,
        320.0,
        RampScale::Linear,
        255,
        &[
            (0.00, [255, 60, 0]),    // ~180 K: extreme overshooting top
            (0.06, [255, 230, 0]),   // ~188 K
            (0.13, [255, 255, 255]), // ~198 K: deep convective cloud tops
            (0.35, [180, 180, 180]), // ~229 K: cold cirrus/anvil
            (0.55, [100, 100, 100]), // ~257 K: mid-level cloud
            (0.75, [40, 40, 40]),    // ~285 K: warm cloud / low stratus
            (1.00, [0, 0, 0]),       // 320 K: clear, warm ground
        ]
    )
};

/// Plain grayscale reflectance, the way every satellite loop shows visible imagery: dark ocean
/// and land, bright cloud. `input_scale` turns the wire's 0..~1.2 reflectance factor into the
/// 0..120 % people read it as; anything past 100 % (a sun-glint or a bright overshoot) just
/// clips white rather than needing its own scale.
static GOES_VISIBLE: FieldRamp = FieldRamp {
    input_scale: 100.0,
    is_temp_kelvin: false,
    ..ramp!(
        "Visible reflectance",
        "%",
        0.0,
        100.0,
        RampScale::Linear,
        255,
        &[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])]
    )
};

/// Water vapor's own enhancement: dark/warm where the mid-upper troposphere is dry (subsidence,
/// often the interesting signal for severe setups), through white, into blue-white for the
/// coldest, moistest air a jet streak or a deep trough drags across the loop.
///
/// Shared by all three water-vapor channels ([`FieldLayer::GoesWaterVapor`] Band 8 upper-level,
/// [`FieldLayer::GoesMidWaterVapor`] Band 9 mid-level, [`FieldLayer::GoesLowWaterVapor`] Band 10
/// lower-level) — the same physical quantity (brightness temperature) and the same forecaster
/// convention for reading it applies at all three levels, so this isn't three ramps that happen
/// to look alike, it's one ramp genuinely shared three ways.
///
/// [`FieldLayer::GoesWaterVapor`]: crate::render::FieldLayer::GoesWaterVapor
/// [`FieldLayer::GoesMidWaterVapor`]: crate::render::FieldLayer::GoesMidWaterVapor
/// [`FieldLayer::GoesLowWaterVapor`]: crate::render::FieldLayer::GoesLowWaterVapor
static GOES_WATER_VAPOR: FieldRamp = FieldRamp {
    input_scale: 1.0,
    is_temp_kelvin: true,
    ..ramp!(
        "Water vapor brightness temp",
        "K",
        200.0,
        260.0,
        RampScale::Linear,
        255,
        &[
            (0.00, [10, 10, 40]),    // 200 K: coldest, moistest upper-level air
            (0.30, [40, 100, 180]),  // 218 K
            (0.55, [200, 220, 230]), // 233 K
            (0.75, [110, 90, 70]),   // 245 K
            (1.00, [30, 20, 10]),    // 260 K: driest, warmest — subsidence
        ]
    )
};

/// Band 7's own enhancement: an ordinary IR-style grayscale from 180-320 K (cloud tops through
/// clear ground — this channel is still perfectly readable as a plain IR loop day or night), then
/// a distinct hot-color ramp from 320-400 K, because this channel's actual reason for existing
/// is fire/hotspot detection — a sub-pixel fire raises Band 7's brightness temperature far more
/// than any longer-wave IR channel, into a range no ordinary cloud or clear-sky pixel reaches.
static GOES_SHORTWAVE_IR: FieldRamp = FieldRamp {
    input_scale: 1.0,
    is_temp_kelvin: true,
    ..ramp!(
        "Shortwave IR brightness temp",
        "K",
        180.0,
        400.0,
        RampScale::Linear,
        255,
        &[
            (0.000, [255, 255, 255]), // 180 K: coldest cloud tops
            (0.180, [180, 180, 180]), // 220 K: cold cirrus/anvil
            (0.364, [90, 90, 90]),    // 260 K: mid-level cloud
            (0.545, [30, 30, 30]),    // 300 K: warm cloud / clear ground
            (0.636, [0, 0, 0]),       // 320 K: warm surface — the ordinary IR scale tops out here
            (0.682, [255, 255, 0]),   // 330 K: fire watch — brightness temp above anything normal
            (0.773, [255, 150, 0]),   // 350 K: fire
            (0.909, [255, 0, 0]),     // 380 K: intense fire
            (1.000, [255, 0, 255]),   // 400 K: saturated hotspot
        ]
    )
};

/// The classic split-window dust/ash detection technique — Band 13 (clean IR) minus Band 15
/// (dirty/split-window IR) brightness temperature, ROADMAP_NEW E6's "channel difference
/// products." Deliberately stored and displayed as Band 13 − Band 15 (not the more commonly
/// quoted Band 15 − Band 13) so the sign works *with* this ramp system's existing "value below
/// `lo` is transparent" cutoff instead of needing a second mechanism: dust/ash makes the dirty
/// channel read *colder* than the clean one, so Band 13 − Band 15 comes out positive over a
/// dust/ash cloud and near-zero (or slightly negative, from ordinary channel noise) everywhere
/// else — `lo` at 1.0 K is the deadband that hides that everywhere-else noise, exactly the way
/// `ModelDiff`'s own hand-rolled deadband LUT hides model agreement, but for free from a plain
/// `Linear` ramp because the physical quantity was chosen so "no signal" and "below the visible
/// range" are the same condition.
static GOES_DUST_DIFF: FieldRamp = FieldRamp {
    input_scale: 1.0,
    is_temp_kelvin: false, // a temperature *difference*, not an absolute reading — no C/F conversion
    ..ramp!(
        "Dust/ash signal (Band 13 \u{2212} Band 15)",
        "K",
        1.0,
        6.0,
        RampScale::Linear,
        220,
        &[
            (0.0, [255, 230, 180]), // 1 K: weakest visible signal — pale tan
            (0.4, [230, 140, 120]), // 3 K: moderate
            (1.0, [200, 40, 140]),  // 6 K: strong dust/ash signal — magenta, the conventional color
        ]
    )
};

static GLOBAL_TEMP_2M: FieldRamp = FieldRamp {
    // Kelvin → °C/°F is an offset, not a scale, so `input_scale` (a pure multiplier) can't do it;
    // the ramp stays in Kelvin and `is_temp_kelvin` has the legend convert to the Units setting.
    input_scale: 1.0,
    is_temp_kelvin: true,
    ..ramp!(
        "2 m temp",
        "K",
        233.0,
        318.0,
        RampScale::Linear,
        170,
        &[
            (0.0, [80, 40, 160]),
            (0.25, [60, 140, 220]),
            (0.5, [230, 230, 210]),
            (0.75, [240, 160, 50]),
            (1.0, [190, 40, 40]),
        ]
    )
};

/// Global 10 m wind speed (the U component's magnitude band, which is what the layer draws).
static GLOBAL_WIND_10M: FieldRamp = FieldRamp {
    input_scale: 1.943_844, // m/s → kt
    ..ramp!(
        "10 m wind",
        "kt",
        5.0,
        80.0,
        RampScale::Abs,
        170,
        &[
            (0.0, [70, 130, 180]),
            (0.4, [90, 200, 140]),
            (0.7, [240, 200, 60]),
            (1.0, [220, 60, 60]),
        ]
    )
};

/// Global moisture: GFS publishes precipitable water, ECMWF total precipitation. Both are
/// millimetres of water and both answer "how wet is this air mass".
static GLOBAL_PRECIP: FieldRamp = ramp!(
    "Precipitable water",
    "mm",
    1.0,
    70.0,
    RampScale::Log,
    170,
    &[
        (0.0, [60, 80, 120]),
        (0.4, [70, 170, 190]),
        (0.7, [90, 210, 110]),
        (1.0, [240, 230, 90]),
    ]
);

/// Forecast snowfall accumulation. The model reports metres; nobody talks in metres of snow.
static SNOWFALL: FieldRamp = FieldRamp {
    input_scale: 39.370_08, // m → in
    ..ramp!(
        "Snowfall",
        "in",
        0.1,
        24.0,
        RampScale::Log,
        220,
        &[
            (0.0, [200, 235, 255]),
            (0.35, [90, 170, 235]),
            (0.6, [60, 90, 210]),
            (0.8, [140, 60, 200]),
            (1.0, [240, 240, 255]),
        ]
    )
};

/// Observed snowfall. Same units and shape as the forecast scale, one decade taller: a 72-hour
/// analysis of a lake-effect band goes places a model run does not.
static SNOW_ANALYSIS: FieldRamp = FieldRamp {
    input_scale: 39.370_08, // m → in
    ..ramp!(
        "Snowfall (observed)",
        "in",
        0.1,
        48.0,
        RampScale::Log,
        220,
        &[
            (0.0, [200, 235, 255]),
            (0.35, [90, 170, 235]),
            (0.6, [60, 90, 210]),
            (0.8, [140, 60, 200]),
            (1.0, [240, 240, 255]),
        ]
    )
};

/// Banded snow. The values are reflectivity, but the scale is not the reflectivity palette: what
/// the layer says is "this echo is organised into a line", and it has to read as that against the
/// mosaic it was cut out of.
static SNOW_BANDS: FieldRamp = ramp!(
    "Snow bands",
    "dBZ",
    10.0,
    45.0,
    RampScale::Linear,
    230,
    &[
        (0.0, [120, 160, 210]),
        (0.5, [180, 215, 250]),
        (1.0, [255, 255, 255]),
    ]
);

/// Calibrated chance of a thunderstorm. A probability, so the scale is linear and the numbers on
/// the legend are the forecast: 30 means thirty percent.
static THUNDER_PROB: FieldRamp = ramp!(
    "Chance of thunder",
    "%",
    5.0,
    100.0,
    RampScale::Linear,
    150,
    &[
        (0.0, [70, 100, 140]),
        (0.35, [90, 180, 190]),
        (0.7, [240, 200, 90]),
        (1.0, [235, 90, 70]),
    ]
);

static PRECIP_TYPE: FieldRamp = FieldRamp {
    label: "Precip type",
    units: "",
    alpha: 200,
    input_scale: 1.0,
    is_temp_kelvin: false,
    scale: FieldScale::Categorical(&[
        (1, [60, 200, 90], "Rain"),
        (3, [90, 150, 240], "Snow"),
        (6, [240, 230, 60], "Convective"),
        (7, [230, 40, 40], "Hail"),
        (10, [40, 200, 200], "Cold rain"),
        (91, [80, 220, 120], "Tropical rain"),
        (96, [80, 220, 120], "Tropical convective"),
    ]),
};

static HCA: FieldRamp = FieldRamp {
    label: "Hydrometeor class",
    units: "",
    alpha: 200,
    input_scale: 1.0,
    is_temp_kelvin: false,
    scale: FieldScale::Categorical(&[
        (10, [140, 110, 90], "Biological"),
        (20, [95, 95, 95], "Clutter"),
        (30, [185, 220, 255], "Ice crystals"),
        (40, [110, 160, 240], "Dry snow"),
        (50, [0, 200, 255], "Wet snow"),
        (60, [90, 200, 90], "Light rain"),
        (70, [25, 145, 50], "Heavy rain"),
        (80, [240, 200, 60], "Big drops"),
        (90, [200, 120, 220], "Graupel"),
        (100, [230, 50, 50], "Hail"),
        (110, [170, 0, 0], "Large hail"),
        (120, [120, 0, 60], "Giant hail"),
        (140, [160, 160, 160], "Unknown"),
        (150, [240, 150, 200], "Range folded"),
    ]),
};

static UPDRAFT_HELICITY: FieldRamp = ramp!(
    "Forecast rotation",
    "m\u{b2}/s\u{b2}",
    25.0,
    200.0,
    RampScale::Linear,
    220,
    &[
        (0.0, [90, 60, 190]),
        (0.35, [160, 60, 220]),
        (0.7, [230, 70, 200]),
        (1.0, [255, 210, 245]),
    ]
);

static SMOKE: FieldRamp = FieldRamp {
    input_scale: 1.0e9,
    ..ramp_smoke()
};

const fn ramp_smoke() -> FieldRamp {
    ramp!(
        "Smoke",
        "\u{b5}g/m\u{b3}",
        2.0,
        150.0,
        RampScale::Log,
        170,
        &[
            (0.0, [170, 170, 165]),
            (0.4, [160, 135, 100]),
            (0.7, [140, 95, 60]),
            (1.0, [90, 50, 30]),
        ]
    )
}

/// Bake a 256-entry RGBA LUT from ramp `stops`, with `alpha` on every data index. Indices 0 and
/// 1 stay clear — [`FieldRamp::index`] emits 0 for "no data" and 2..=255 for t in 0..=1, so the
/// bake must use the same mapping or every color sits a hair off its value.
/// Shared by the GPU upload path and the headless verifiers.
pub fn bake_ramp_lut(stops: &[(f32, [u8; 3])], alpha: u8) -> Vec<u8> {
    let mut lut = vec![0u8; 256 * 4];
    for (i, slot) in lut.as_chunks_mut::<4>().0.iter_mut().enumerate().skip(2) {
        let t = (i - 2) as f32 / 253.0;
        let mut rgb = stops[0].1;
        for w in stops.windows(2) {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            if t >= t0 && t <= t1 {
                let u = if (t1 - t0).abs() < f32::EPSILON {
                    0.0
                } else {
                    (t - t0) / (t1 - t0)
                };
                rgb = [
                    (c0[0] as f32 + (c1[0] as f32 - c0[0] as f32) * u) as u8,
                    (c0[1] as f32 + (c1[1] as f32 - c0[1] as f32) * u) as u8,
                    (c0[2] as f32 + (c1[2] as f32 - c0[2] as f32) * u) as u8,
                ];
                break;
            }
            if t > t1 {
                rgb = c1;
            }
        }
        slot.copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    }
    lut
}

/// The color scale for `layer`, or `None` for layers colored some other way: the reflectivity
/// palette (`Mrms`/`Hrrr`, which follow the user's `.pal` table) and `Lightning` (own upload fn).
pub fn ramp_for(layer: FieldLayer) -> Option<&'static FieldRamp> {
    use FieldLayer as FL;
    if let Some(field) = layer.descriptor() {
        use wxdata::field::PaletteId;
        return match field.default_palette {
            PaletteId::Reflectivity | PaletteId::LightningDensity => None,
            PaletteId::Rotation => Some(&ROTATION),
            PaletteId::HailSize => Some(&MESH),
            PaletteId::HailSwath => Some(&HAIL_SWATH),
            PaletteId::PrecipitationRate => Some(&PRECIP_RATE),
            PaletteId::Precipitation1h => Some(&QPE_1H),
            PaletteId::PrecipitationAccum => Some(&QPE_MID),
            PaletteId::Precipitation24h => Some(&QPE_24H),
            PaletteId::PrecipitationType => Some(&PRECIP_TYPE),
            PaletteId::FloodRecurrence => Some(&FLASH_FLOOD),
            PaletteId::Cape => Some(&CAPE),
            PaletteId::Helicity => Some(&SRH),
            PaletteId::UpdraftHelicity => Some(&UPDRAFT_HELICITY),
            PaletteId::Snowfall => Some(&SNOWFALL),
            PaletteId::ThunderProbability => Some(&THUNDER_PROB),
            PaletteId::Smoke => Some(&SMOKE),
            PaletteId::MeanSeaLevelPressure => Some(&GLOBAL_MSLP),
            PaletteId::Temperature => Some(&GLOBAL_TEMP_2M),
            PaletteId::Dewpoint => Some(&GLOBAL_DEWPOINT_2M),
            PaletteId::Height500 => Some(&GLOBAL_HEIGHT_500),
            PaletteId::Wind10m => Some(&GLOBAL_WIND_10M),
            PaletteId::PrecipitableWater => Some(&GLOBAL_PRECIP),
        };
    }
    Some(match layer {
        FL::Cape => &CAPE,
        FL::Srh => &SRH,
        // Locally derived twins share their L3 counterparts' scales — one VIL scale app-wide, so
        // a number means the same thing whichever source drew it.
        FL::Vil | FL::VilLocal => &VIL,
        FL::EchoTops | FL::EtopLocal => &ECHO_TOPS,
        FL::VilDensity => &VIL_DENSITY,
        // MEHS shares the MRMS MESH scale: one hail scale app-wide.
        FL::HailMehs => &MESH,
        FL::HailPosh => &POSH,
        FL::UpdraftHelicity => &UPDRAFT_HELICITY,
        FL::Smoke => &SMOKE,
        FL::Snowfall => &SNOWFALL,
        FL::SnowAnalysis => &SNOW_ANALYSIS,
        FL::GlobalMslp => &GLOBAL_MSLP,
        FL::GlobalHeight500 => &GLOBAL_HEIGHT_500,
        FL::GlobalTemp2m => &GLOBAL_TEMP_2M,
        FL::GlobalDewpoint2m => &GLOBAL_DEWPOINT_2M,
        FL::GlobalWind10m => &GLOBAL_WIND_10M,
        FL::GlobalPrecip => &GLOBAL_PRECIP,
        FL::Hca => &HCA,
        FL::GlmFed => &GLM_FED,
        FL::SnowBands => &SNOW_BANDS,
        FL::ThunderProb => &THUNDER_PROB,
        FL::GoesIr => &GOES_IR,
        // Reads near-identically to clean IR on its own — its real value is as the other half of
        // a split-window (Band 15 minus Band 13) dust/ash difference product, not yet built
        // (ROADMAP_NEW E6) — so sharing the ramp here rather than tabulating a visually
        // indistinguishable second one is honest, not a shortcut.
        FL::GoesDirtyIr => &GOES_IR,
        FL::GoesVisible => &GOES_VISIBLE,
        // Same physical quantity and the same forecaster reading convention at all three
        // altitudes — see GOES_WATER_VAPOR's own doc comment for why this is a genuine three-way
        // share, not three ramps that happen to coincide.
        FL::GoesWaterVapor | FL::GoesMidWaterVapor | FL::GoesLowWaterVapor => &GOES_WATER_VAPOR,
        FL::GoesShortwaveIr => &GOES_SHORTWAVE_IR,
        FL::GoesDustDiff => &GOES_DUST_DIFF,
        // Same physical quantity and units as the global-model equivalents (NDFD publishes
        // Kelvin/m/s/metres same as every other model this app reads), so they share the ramp
        // rather than tabulating a second, identical one.
        FL::NdfdTemp2m => &GLOBAL_TEMP_2M,
        FL::NdfdWind10m | FL::NdfdGust10m => &GLOBAL_WIND_10M,
        FL::NdfdSnow => &SNOWFALL,
        // Composite is reflectivity in dBZ, so like the mosaic it follows the user's own
        // reflectivity `.pal` rather than a fixed ramp of its own.
        // The compare panes borrow their ramp from whichever single-model layer shares their
        // field's physical units (`DiffField::source_layer`) instead of tabulating a second one
        // here — same reason `ModelDiff` has none of its own (built dynamically in `fielddiff`).
        FL::Mrms
        | FL::Rotation
        | FL::AzShear
        | FL::Mesh
        | FL::HailSwath
        | FL::PrecipRate
        | FL::Qpe1h
        | FL::Qpe3h
        | FL::Qpe6h
        | FL::Qpe12h
        | FL::Qpe24h
        | FL::FlashFlood
        | FL::PrecipType
        | FL::Mosaic
        | FL::CompositeLocal
        | FL::Hrrr
        | FL::Lightning
        | FL::ModelDiff
        | FL::CompareA
        | FL::CompareB => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_palettes_preserve_existing_scales() {
        for (id, expected) in [
            ("rotation", &ROTATION),
            ("azshear", &ROTATION),
            ("mesh", &MESH),
            ("hailswath", &HAIL_SWATH),
            ("preciprate", &PRECIP_RATE),
            ("qpe1h", &QPE_1H),
            ("qpe3h", &QPE_MID),
            ("qpe6h", &QPE_MID),
            ("qpe12h", &QPE_MID),
            ("qpe24h", &QPE_24H),
            ("preciptype", &PRECIP_TYPE),
            ("flashflood", &FLASH_FLOOD),
        ] {
            let layer = FieldLayer::from_slug(id).unwrap();
            assert!(std::ptr::eq(ramp_for(layer).unwrap(), expected), "{id}");
        }
        // These retain their user-configured and density-specific upload paths.
        assert!(ramp_for(FieldLayer::Mrms).is_none());
        assert!(ramp_for(FieldLayer::Lightning).is_none());
    }

    #[test]
    fn model_catalog_palettes_preserve_existing_scales() {
        for (layer, expected) in [
            (FieldLayer::Cape, &CAPE),
            (FieldLayer::Srh, &SRH),
            (FieldLayer::UpdraftHelicity, &UPDRAFT_HELICITY),
            (FieldLayer::Snowfall, &SNOWFALL),
            (FieldLayer::ThunderProb, &THUNDER_PROB),
            (FieldLayer::Smoke, &SMOKE),
        ] {
            assert!(layer.descriptor().is_some(), "{layer:?}");
            assert!(
                std::ptr::eq(ramp_for(layer).unwrap(), expected),
                "{layer:?}"
            );
        }
        assert!(FieldLayer::Hrrr.descriptor().is_some());
        assert!(ramp_for(FieldLayer::Hrrr).is_none());
    }

    /// Phase A1's global-model migration: the six `Global*` layers must resolve through
    /// `GlobalField::descriptor` (provenance, search, palette) rather than the old bare
    /// fallback-match arms, while still drawing with the exact same ramp those arms named —
    /// migrating the metadata must not silently swap in a different color scale.
    #[test]
    fn global_catalog_palettes_preserve_existing_scales() {
        for (layer, expected) in [
            (FieldLayer::GlobalMslp, &GLOBAL_MSLP),
            (FieldLayer::GlobalHeight500, &GLOBAL_HEIGHT_500),
            (FieldLayer::GlobalTemp2m, &GLOBAL_TEMP_2M),
            (FieldLayer::GlobalDewpoint2m, &GLOBAL_DEWPOINT_2M),
            (FieldLayer::GlobalWind10m, &GLOBAL_WIND_10M),
            (FieldLayer::GlobalPrecip, &GLOBAL_PRECIP),
        ] {
            assert!(layer.descriptor().is_some(), "{layer:?}");
            assert!(
                std::ptr::eq(ramp_for(layer).unwrap(), expected),
                "{layer:?}"
            );
        }
    }

    /// Only the two Kelvin-wire fields ask the legend to convert; every other ramp's `lo`/`hi`
    /// are already in the units it labels itself with, and must stay untouched by the flag.
    #[test]
    fn only_the_kelvin_ramps_ask_for_temperature_conversion() {
        assert!(GLOBAL_TEMP_2M.is_temp_kelvin, "labelled K, must convert");
        assert!(
            GLOBAL_DEWPOINT_2M.is_temp_kelvin,
            "labelled K, must convert"
        );
        for (name, r) in [
            ("MSLP", &GLOBAL_MSLP),
            ("500mb height", &GLOBAL_HEIGHT_500),
            ("10m wind", &GLOBAL_WIND_10M),
            ("wind particles", &WIND),
            ("snowfall", &SNOWFALL),
        ] {
            assert!(!r.is_temp_kelvin, "{name} is not a temperature ramp");
        }
    }

    /// Layers colored outside this table. A new `FieldLayer` must join the table or this list —
    /// forgetting both silently ships a layer with no legend.
    const NO_RAMP: [FieldLayer; 8] = [
        FieldLayer::Mrms,
        FieldLayer::Mosaic,
        FieldLayer::CompositeLocal,
        FieldLayer::Hrrr,
        FieldLayer::Lightning,
        // The difference layer's ramp is symmetric about zero and rebuilt whenever the field
        // changes, so it is baked in `fielddiff`, not tabulated here.
        FieldLayer::ModelDiff,
        // The compare panes reuse whichever single-model layer's ramp matches their field
        // (`DiffField::source_layer`) rather than owning one, so their own legend draw is
        // special-cased in `render_pane` instead of going through `ramp_for` at all.
        FieldLayer::CompareA,
        FieldLayer::CompareB,
    ];

    #[test]
    fn every_layer_is_either_ramped_or_explicitly_exempt() {
        for l in FieldLayer::DRAW_ORDER {
            let exempt = NO_RAMP.contains(&l);
            assert_eq!(
                ramp_for(l).is_some(),
                !exempt,
                "{l:?} must have a ramp or be listed in NO_RAMP"
            );
        }
    }

    #[test]
    fn ramps_are_labeled_and_ordered() {
        for l in FieldLayer::DRAW_ORDER {
            let Some(r) = ramp_for(l) else { continue };
            assert!(!r.label.is_empty(), "{l:?}");
            if let FieldScale::Ramp { lo, hi, .. } = r.scale {
                assert!(lo < hi, "{l:?}: lo {lo} must be below hi {hi}");
            }
        }
    }

    #[test]
    fn index_maps_endpoints() {
        let m = ramp_for(FieldLayer::Mesh).unwrap();
        assert_eq!(m.index(9.9), 0, "below threshold is transparent");
        assert_eq!(m.index(10.0), 2, "threshold is the first visible index");
        assert_eq!(m.index(75.0), 255);
        assert_eq!(m.index(9999.0), 255, "clamps above the top");
    }

    #[test]
    fn abs_scale_ignores_sign() {
        let r = ramp_for(FieldLayer::AzShear).unwrap();
        assert_eq!(r.index(-20.0), r.index(20.0));
        assert_eq!(r.index(-1.0), 0);
    }

    #[test]
    fn log_scale_is_monotonic_across_decades() {
        let q = ramp_for(FieldLayer::Qpe1h).unwrap();
        assert_eq!(q.index(0.2), 0);
        let (a, b, c) = (q.index(1.0), q.index(10.0), q.index(100.0));
        assert!(a < b && b < c, "{a} {b} {c}");
        assert_eq!(c, 255);
    }

    #[test]
    fn categorical_index_is_the_raw_class_code() {
        let h = ramp_for(FieldLayer::Hca).unwrap();
        assert_eq!(h.index(110.0), 110);
    }

    #[test]
    fn the_shared_goes_ramps_really_are_the_same_ramp() {
        // Pointer equality, not just value equality — these are documented as one `static` shared
        // three/two ways, not three/two ramps that happen to have identical fields.
        let wv = ramp_for(FieldLayer::GoesWaterVapor).unwrap();
        let mid_wv = ramp_for(FieldLayer::GoesMidWaterVapor).unwrap();
        let low_wv = ramp_for(FieldLayer::GoesLowWaterVapor).unwrap();
        assert!(std::ptr::eq(wv, mid_wv));
        assert!(std::ptr::eq(wv, low_wv));

        let ir = ramp_for(FieldLayer::GoesIr).unwrap();
        let dirty_ir = ramp_for(FieldLayer::GoesDirtyIr).unwrap();
        assert!(std::ptr::eq(ir, dirty_ir));
    }

    #[test]
    fn shortwave_ir_distinguishes_a_fire_hotspot_from_ordinary_cloud() {
        let r = ramp_for(FieldLayer::GoesShortwaveIr).unwrap();
        let cold_cloud = r.index(220.0);
        let clear_ground = r.index(300.0);
        let fire = r.index(360.0);
        assert!(
            cold_cloud < clear_ground,
            "cold cloud must read cooler than clear ground on the ordinary IR portion"
        );
        assert!(
            fire > clear_ground,
            "a fire-range brightness temperature must read hotter than any ordinary ground \
             pixel, not fall back into the same grayscale band"
        );
        assert_eq!(r.index(400.0), 255, "clamps at the saturated-hotspot end");
    }

    #[test]
    fn dust_diff_hides_ordinary_noise_but_shows_a_real_signal() {
        let r = ramp_for(FieldLayer::GoesDustDiff).unwrap();
        // No dust: the two channels read within a fraction of a degree of each other, or Band 15
        // even reads slightly warmer (a small negative Band13-Band15 value) — both must vanish.
        assert_eq!(
            r.index(0.3),
            0,
            "ordinary channel noise must not paint as a signal"
        );
        assert_eq!(
            r.index(-0.5),
            0,
            "a slightly negative value is not a signal either"
        );
        // A real dust/ash signal clears the deadband and actually renders.
        assert!(
            r.index(3.0) > 0,
            "a real signal must be visible, not swallowed by the deadband"
        );
        assert!(
            r.index(6.0) >= r.index(3.0),
            "a stronger signal must read at least as intense, not dimmer"
        );
    }

    #[test]
    fn lut_uses_the_encoder_index_mapping() {
        let stops = [(0.0, [10, 20, 30]), (1.0, [200, 210, 220])];
        let lut = bake_ramp_lut(&stops, 255);
        assert_eq!(&lut[2 * 4..2 * 4 + 3], &[10, 20, 30], "index 2 is t=0");
        assert_eq!(
            &lut[255 * 4..255 * 4 + 3],
            &[200, 210, 220],
            "index 255 is t=1"
        );
        assert_eq!(lut[4 + 3], 0, "index 1 is never emitted, stays clear");
    }
}
