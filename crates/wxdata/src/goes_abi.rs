//! GOES ABI Level 2 Cloud and Moisture Imagery (CMIP), read directly from the public
//! `noaa-goes{18,19}` S3 buckets instead of GIBS' pre-rendered tiles.
//!
//! ABI CMIP is netCDF-4, which is HDF5 underneath — [`hdf5lite`] already reads it for GLM
//! lightning. The one new piece is geometry: the satellite doesn't scan a lat/lon grid, it scans
//! fixed angles off its own line of sight, so every pixel needs the GOES-R fixed-grid geostationary
//! projection to find out where on Earth it actually is. [`decode`] forward-projects every
//! source pixel once and bins it into a regular lat/lon grid — the same shape [`MrmsField`] already
//! gives every other gridded overlay — rather than solving the (markedly fiddlier) inverse problem
//! of which source pixel lands under each output cell.

use crate::mrms::MrmsField;

/// GOES-East (operational since April 2025) and GOES-West, by their public S3 bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Satellite {
    East,
    West,
}

impl Satellite {
    /// The bucket of the satellite that held this position at time `t`: GOES-19 has been East
    /// since 7 April 2025 (GOES-16 before), GOES-18 West since 4 January 2023 (GOES-17 before), so
    /// an archive case from 2021 reads the satellite that actually took it.
    fn bucket_at(self, t: chrono::DateTime<chrono::Utc>) -> &'static str {
        let since = |y, m, d| {
            chrono::NaiveDate::from_ymd_opt(y, m, d)
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc())
        };
        match self {
            Satellite::East if since(2025, 4, 7).is_some_and(|s| t < s) => {
                "https://noaa-goes16.s3.amazonaws.com"
            }
            Satellite::East => "https://noaa-goes19.s3.amazonaws.com",
            Satellite::West if since(2023, 1, 4).is_some_and(|s| t < s) => {
                "https://noaa-goes17.s3.amazonaws.com"
            }
            Satellite::West => "https://noaa-goes18.s3.amazonaws.com",
        }
    }
}

/// Which ABI scan a granule comes from. CONUS: every ~5 minutes from both satellites, covering
/// the whole area this app's other overlays care about. A mesoscale sector: a box about 1000 km
/// on a side, scanned every minute, that NOAA points at whatever is happening (ROADMAP_NEW E5's
/// rapid scan) and moves as the event does — so where it is has to be read from each granule
/// ([`Footprint`]), never assumed. Full disk (10-minute cadence, mostly ocean for a US-focused
/// app) is not read.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Sector {
    #[default]
    Conus,
    Meso1,
    Meso2,
}

impl Sector {
    pub const ALL: [Sector; 3] = [Sector::Conus, Sector::Meso1, Sector::Meso2];

    /// The S3 product directory.
    fn product(self) -> &'static str {
        match self {
            Sector::Conus => "ABI-L2-CMIPC",
            Sector::Meso1 | Sector::Meso2 => "ABI-L2-CMIPM",
        }
    }

    /// The product as the file names spell it (the mesoscale sectors add their number).
    fn file_product(self) -> &'static str {
        match self {
            Sector::Conus => "ABI-L2-CMIPC",
            Sector::Meso1 => "ABI-L2-CMIPM1",
            Sector::Meso2 => "ABI-L2-CMIPM2",
        }
    }

    /// How often a new scan lands.
    pub fn cadence_secs(self) -> u64 {
        match self {
            Sector::Conus => 300,
            Sector::Meso1 | Sector::Meso2 => 60,
        }
    }

    pub fn is_meso(self) -> bool {
        self != Sector::Conus
    }

    pub fn label(self) -> &'static str {
        match self {
            Sector::Conus => "CONUS (5 min)",
            Sector::Meso1 => "Mesoscale 1 (1 min)",
            Sector::Meso2 => "Mesoscale 2 (1 min)",
        }
    }
}

/// Where a mesoscale sector is pointed now: the lat/lon box its newest granule covers, and when
/// that scan began.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Footprint {
    pub lon_west: f64,
    pub lon_east: f64,
    pub lat_south: f64,
    pub lat_north: f64,
    pub time: chrono::DateTime<chrono::Utc>,
}

impl Footprint {
    /// The box a decoded granule covers.
    pub fn of(field: &MrmsField) -> Footprint {
        Footprint {
            lon_west: field.lon_west,
            lon_east: field.lon_east,
            lat_south: field.lat_south,
            lat_north: field.lat_north,
            time: field.time,
        }
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        (self.lon_west..=self.lon_east).contains(&lon)
            && (self.lat_south..=self.lat_north).contains(&lat)
    }
}

/// The GOES-R fixed-grid geostationary projection, read off a granule's `goes_imager_projection`
/// attributes. Same for every file from the same satellite; re-derived per fetch rather than
/// hardcoded since GOES-East and -West don't share one (different `longitude_of_projection_origin`).
#[derive(Debug, Clone, Copy)]
struct Projection {
    /// Semi-major axis of the reference ellipsoid (m).
    req: f64,
    /// Semi-minor axis (m).
    rpol: f64,
    /// Distance from the Earth's center to the satellite (m) — `perspective_point_height` is
    /// measured from the ellipsoid surface, so this adds `req` to get the value the projection
    /// formula actually wants.
    h: f64,
    /// Longitude the satellite sits over (radians).
    lon_origin: f64,
}

impl Projection {
    fn from_attrs(attrs: &std::collections::HashMap<String, hdf5lite::Value>) -> Option<Self> {
        let get = |k: &str| attrs.get(k).and_then(hdf5lite::Value::as_f64);
        let req = get("semi_major_axis")?;
        let rpol = get("semi_minor_axis")?;
        let height = get("perspective_point_height")?;
        let lon_origin = get("longitude_of_projection_origin")?;
        Some(Projection {
            req,
            rpol,
            h: height + req,
            lon_origin: lon_origin.to_radians(),
        })
    }

    /// Fixed-grid scan angles (radians) to geodetic (lon, lat) in degrees, or `None` for a scan
    /// angle that misses the Earth's disk entirely (space, past the limb).
    ///
    /// The NOAA GOES-R Product Definition and Users' Guide's navigation equations, direction
    /// "elevation/scan angle to geodetic": <https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf>
    /// (section 4.2.8.1), the same formula `satpy`/`goes2go`/NOAA's own sample scripts use.
    fn scan_to_lonlat(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (sin_x, cos_x) = x.sin_cos();
        let (sin_y, cos_y) = y.sin_cos();
        let ecc2 = (self.req / self.rpol).powi(2);
        let a = sin_x * sin_x + cos_x * cos_x * (cos_y * cos_y + ecc2 * sin_y * sin_y);
        let b = -2.0 * self.h * cos_x * cos_y;
        let c = self.h * self.h - self.req * self.req;
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None; // off the visible disk
        }
        let rs = (-b - disc.sqrt()) / (2.0 * a);
        let sx = rs * cos_x * cos_y;
        let sy = -rs * sin_x;
        let sz = rs * cos_x * sin_y;
        let lat = (ecc2 * (sz / ((self.h - sx).powi(2) + sy * sy).sqrt())).atan();
        let lon = self.lon_origin - (sy / (self.h - sx)).atan();
        Some((lon.to_degrees(), lat.to_degrees()))
    }
}

/// Decode one ABI CMIP granule's `CMI` band into a regular lat/lon [`MrmsField`], resampled onto
/// an `out_nx × out_ny` grid spanning the scene's own projected extent.
///
/// Nearest-source-pixel-wins scatter, not the other direction (walking the output grid and
/// inverse-projecting each cell back to a source pixel): the inverse fixed-grid transform is
/// available, but this way only ever needs the forward one, which the sub-satellite-point identity
/// (`x=y=0` reads back the projection origin exactly) is enough to check by hand. A source pixel
/// this dense on a CONUS-sized output grid leaves at most a handful of unclaimed cells, which stay
/// `NaN` like any other gap this app's grids already carry.
pub fn decode(bytes: Vec<u8>, out_nx: usize, out_ny: usize) -> anyhow::Result<MrmsField> {
    decode_var(bytes, "CMI", out_nx, out_ny)
}

/// [`decode`] for any variable on the ABI fixed grid — the Level 2 products carry theirs under
/// their own names (`HT`, cloud top height, in the ACHA product), with the same projection, `x`/`y`,
/// `t` and `DQF` layout as CMIP.
pub fn decode_var(
    bytes: Vec<u8>,
    var: &str,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let f = hdf5lite::File::open(bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    let proj_attrs = f
        .attributes("goes_imager_projection")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let proj = Projection::from_attrs(&proj_attrs)
        .ok_or_else(|| anyhow::anyhow!("goes_imager_projection missing required attributes"))?;

    let x = f.read_f64("x").map_err(|e| anyhow::anyhow!("x: {e}"))?;
    let y = f.read_f64("y").map_err(|e| anyhow::anyhow!("y: {e}"))?;
    let mut cmi = f.read_f64(var).map_err(|e| anyhow::anyhow!("{var}: {e}"))?;
    let (nx, ny) = (x.len(), y.len());
    anyhow::ensure!(
        cmi.len() == nx * ny,
        "{var} is {} values, expected {nx}x{ny}={}",
        cmi.len(),
        nx * ny
    );
    // Per-pixel quality flags (ROADMAP_NEW E2's "preserve DQF/quality masks where practical") —
    // read failure (an unexpectedly shaped or absent `DQF`, not expected for a real CMIP granule
    // but this reader has no way to be sure of every file a bucket might ever serve) degrades to
    // no masking rather than failing the whole decode: today's un-masked behavior, not a
    // regression.
    if let Ok(dqf) = f.read_f64("DQF") {
        mask_by_dqf(&mut cmi, &dqf);
    }
    // `t`: seconds since the ABI epoch (2000-01-01 12:00:00 UTC), per the dataset's own `units`
    // attribute — falls back to now if the granule is somehow missing it.
    let time = abi_epoch()
        .and_then(|epoch| {
            let t = f.read_f64("t").ok()?;
            epoch.checked_add_signed(chrono::Duration::seconds(*t.first()? as i64))
        })
        .unwrap_or_else(chrono::Utc::now);

    // Forward-project every source pixel once, then bin it into whichever output cell it lands
    // in — the output grid's bounds are the scene's own projected extent, found in the same pass.
    let mut lonlat = Vec::with_capacity(nx * ny);
    let (mut lon_min, mut lon_max, mut lat_min, mut lat_max) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for (row, &yv) in y.iter().enumerate() {
        for (col, &xv) in x.iter().enumerate() {
            let v = cmi[row * nx + col];
            let ll = if v.is_finite() {
                proj.scan_to_lonlat(xv, yv)
            } else {
                None
            };
            if let Some((lon, lat)) = ll {
                lon_min = lon_min.min(lon);
                lon_max = lon_max.max(lon);
                lat_min = lat_min.min(lat);
                lat_max = lat_max.max(lat);
            }
            lonlat.push(ll.map(|(lon, lat)| (lon, lat, v)));
        }
    }
    anyhow::ensure!(
        lon_max > lon_min && lat_max > lat_min,
        "no on-disk pixels decoded"
    );

    let (out_nx, out_ny) = (out_nx.max(2), out_ny.max(2));
    let mut values = vec![f32::NAN; out_nx * out_ny];
    let dlon = (lon_max - lon_min) / out_nx as f64;
    let dlat = (lat_max - lat_min) / out_ny as f64;
    for entry in lonlat.into_iter().flatten() {
        let (lon, lat, v) = entry;
        let col = (((lon - lon_min) / dlon) as usize).min(out_nx - 1);
        // Row 0 is the northernmost latitude, matching MrmsField's own convention.
        let row = (((lat_max - lat) / dlat) as usize).min(out_ny - 1);
        values[row * out_nx + col] = v as f32;
    }

    Ok(MrmsField {
        values,
        nx: out_nx,
        ny: out_ny,
        lon_west: lon_min,
        lon_east: lon_max,
        lat_north: lat_max,
        lat_south: lat_min,
        time,
    })
}

/// Zero out (to `NaN`) every `cmi` value whose corresponding `dqf` flag is not 0 ("good") — 1
/// conditionally usable, 2 out of range, 3 no value, per the ABI Product Definition and Users'
/// Guide. A quantitative reading (this app shows raw brightness temperature/reflectance, not just
/// a rendered picture) only trusts DQF 0; feeding a masked pixel `NaN` routes it through the exact
/// same "no data" path CMI's own fill value already takes, so a bad or missing pixel becomes
/// indistinguishable from off-disk — both correctly transparent — rather than reading as a
/// spurious cold/warm value. A shape mismatch (the two arrays should always be the same length for
/// a real granule) is a no-op rather than a panic or partial mask: [`decode`]'s caller only has
/// this as a best-effort quality improvement, not a correctness requirement it should fail over.
fn mask_by_dqf(cmi: &mut [f64], dqf: &[f64]) {
    if cmi.len() != dqf.len() {
        return;
    }
    for (v, &flag) in cmi.iter_mut().zip(dqf) {
        if flag != 0.0 {
            *v = f64::NAN;
        }
    }
}

/// The epoch ABI's `t` variable counts seconds from (2000-01-01 12:00:00 UTC), per its own `units`
/// attribute.
fn abi_epoch() -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::NaiveDate::from_ymd_opt(2000, 1, 1)?
        .and_hms_opt(12, 0, 0)
        .map(|dt| dt.and_utc())
}

/// Every `<Key>` in an S3 ListObjectsV2 XML response, in the order S3 returned them.
fn all_keys(xml: &str) -> Vec<String> {
    xml.match_indices("<Key>")
        .filter_map(|(i, _)| {
            let rest = &xml[i + 5..];
            rest.find("</Key>").map(|e| rest[..e].to_string())
        })
        .collect()
}

/// A CMIP filename's own scan start time, parsed from its `_s<year(4)><day-of-year(3)><hour(2)>
/// <min(2)><sec(2)><tenth(1)>_` field — e.g. `..._s20262621801173_e...` is 2026-262 (day of year)
/// 18:01:17.3 UTC. Confirmed against a real listing from the live bucket, not assumed from
/// documentation alone (see this module's own `#[ignore = "network"]` tests).
pub(crate) fn key_time(key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let i = key.find("_s")?;
    let digits = key.get(i + 2..i + 2 + 14)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = digits[0..4].parse().ok()?;
    let day_of_year: u32 = digits[4..7].parse().ok()?;
    let hour: u32 = digits[7..9].parse().ok()?;
    let minute: u32 = digits[9..11].parse().ok()?;
    let second: u32 = digits[11..13].parse().ok()?;
    chrono::NaiveDate::from_yo_opt(year, day_of_year)?
        .and_hms_opt(hour, minute, second)
        .map(|dt| dt.and_utc())
}

/// Hour listings already read, by listing URL: `(when read, keys)`. A listing for an hour that had
/// ended well before it was read never changes (scan keys are immutable), so it is kept; one for
/// the hour still filling is reused only for a few seconds. Bounded; a satellite loop steps through
/// the same few hours once per frame, which without this would be three S3 listings a frame.
type ListingCache = std::collections::HashMap<String, (chrono::DateTime<chrono::Utc>, Vec<String>)>;
static LISTINGS: std::sync::Mutex<Option<ListingCache>> = std::sync::Mutex::new(None);
const LISTING_CACHE_MAX: usize = 256;
/// How long a listing of an hour that may still be filling is reused.
const OPEN_HOUR_REUSE_SECS: i64 = 20;
/// How long after an hour ends before its listing is treated as complete: late files land a
/// couple of minutes after their scan.
const HOUR_SETTLE_SECS: i64 = 15 * 60;

/// Whether a listing of the hour starting `hour_start`, read at `read_at`, can still be used at
/// `now`.
fn listing_fresh(
    hour_start: chrono::DateTime<chrono::Utc>,
    read_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let settled = hour_start + chrono::Duration::seconds(3600 + HOUR_SETTLE_SECS);
    read_at >= settled || (now - read_at).num_seconds() < OPEN_HOUR_REUSE_SECS
}

/// Every CMIP key for every band on `satellite`'s `sector` in the UTC hour `t` falls in: one
/// listing request (or the cached one), empty for an hour nothing has landed in.
async fn list_hour(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    t: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<String>> {
    use chrono::{Datelike, Timelike};
    // Through the scan mode letter only: scans before April 2019 are mode 3 (`-M3C13_`), later
    // ones mode 6, so the band is picked out of the listing rather than the prefix.
    let prefix = format!(
        "{}/{:04}/{:03}/{:02}/OR_{}-M",
        sector.product(),
        t.year(),
        t.ordinal(),
        t.hour(),
        sector.file_product(),
    );
    // Every band's scans for the hour: 16 x 60 for a mesoscale sector (CONUS 16 x 12).
    let url = format!(
        "{}/?list-type=2&prefix={prefix}&max-keys=1000",
        satellite.bucket_at(t)
    );
    let hour_start = t
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(t);
    let now = chrono::Utc::now();
    if let Some((read_at, keys)) = LISTINGS
        .lock()
        .ok()
        .and_then(|c| c.as_ref().and_then(|c| c.get(&url).cloned()))
    {
        if listing_fresh(hour_start, read_at, now) {
            return Ok(keys);
        }
    }
    let xml = client
        .get(crate::net::fetch_url(&url))
        .timeout(crate::net::FEED_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let keys = all_keys(&xml);
    if let Ok(mut cache) = LISTINGS.lock() {
        let cache = cache.get_or_insert_with(Default::default);
        if cache.len() >= LISTING_CACHE_MAX {
            // Drop the oldest reads first.
            let mut by_age: Vec<_> = cache.iter().map(|(k, (at, _))| (*at, k.clone())).collect();
            by_age.sort();
            for (_, k) in by_age.into_iter().take(LISTING_CACHE_MAX / 4) {
                cache.remove(&k);
            }
        }
        cache.insert(url, (now, keys.clone()));
    }
    Ok(keys)
}

/// Every CMIP key for `band` on `satellite` in the UTC hour `t` falls in — silently empty (not an
/// error) for an hour nothing has landed in yet, or ever will have.
async fn keys_in_hour(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
    t: chrono::DateTime<chrono::Utc>,
) -> Vec<String> {
    let band_tag = format!("C{band:02}_G");
    list_hour(client, satellite, sector, t)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|k| k.contains(&band_tag))
        .collect()
}

/// One scan of one band in a native listing (ROADMAP_PARITY M5.2): when it began, and its object.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanFrame {
    pub start: chrono::DateTime<chrono::Utc>,
    pub key: String,
}

/// `band`'s scans among `keys`, oldest first, one per scan start (a re-delivered file keeps the
/// first key).
pub fn scan_frames_from_keys(keys: &[String], band: u8) -> Vec<ScanFrame> {
    let band_tag = format!("C{band:02}_G");
    let mut frames: Vec<ScanFrame> = keys
        .iter()
        .filter(|k| k.contains(&band_tag))
        .filter_map(|k| {
            key_time(k).map(|start| ScanFrame {
                start,
                key: k.clone(),
            })
        })
        .collect();
    frames.sort_by_key(|f| f.start);
    frames.dedup_by_key(|f| f.start);
    frames
}

/// A hole in a scan sequence: no scan between `after` and `before` although the sector's cadence
/// says `missing` should have landed there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanGap {
    pub after: chrono::DateTime<chrono::Utc>,
    pub before: chrono::DateTime<chrono::Utc>,
    pub missing: u32,
}

/// The holes in `frames` (oldest first) for a sector scanning every `cadence_secs`: any step
/// longer than one and a half cadences, counted in whole missing scans. Shown on the timeline as
/// gaps rather than closed up, so a loop never implies continuity it does not have.
pub fn scan_gaps(frames: &[ScanFrame], cadence_secs: u64) -> Vec<ScanGap> {
    let cadence = cadence_secs.max(1) as f64;
    frames
        .windows(2)
        .filter_map(|w| {
            let step = (w[1].start - w[0].start).num_seconds() as f64;
            (step > 1.5 * cadence).then(|| ScanGap {
                after: w[0].start,
                before: w[1].start,
                missing: ((step / cadence).round() as u32).saturating_sub(1).max(1),
            })
        })
        .collect()
}

/// `band`'s scans on `satellite`'s `sector` that began in `from..=to`, oldest first, from one
/// listing per UTC hour (cached, see [`list_hour`]). An error only when every hour's listing
/// failed: a quiet hour is an empty answer, not a failure.
pub async fn scan_frames(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<ScanFrame>> {
    use chrono::Timelike;
    anyhow::ensure!(to >= from, "the window ends before it starts");
    anyhow::ensure!(
        to - from <= chrono::Duration::hours(6),
        "a scan listing covers at most six hours"
    );
    let mut hour = from
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(from);
    let (mut keys, mut ok, mut last_err) = (Vec::new(), false, None);
    while hour <= to {
        match list_hour(client, satellite, sector, hour).await {
            Ok(k) => {
                ok = true;
                keys.extend(k);
            }
            Err(e) => last_err = Some(e),
        }
        hour += chrono::Duration::hours(1);
    }
    if !ok {
        return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no listing")));
    }
    Ok(scan_frames_from_keys(&keys, band)
        .into_iter()
        .filter(|f| f.start >= from && f.start <= to)
        .collect())
}

/// Download `key` into the object cache without decoding it, so a loop's later frames load from
/// disk (IndexedDB in the browser) instead of the network.
pub async fn prefetch_key(
    client: &reqwest::Client,
    satellite: Satellite,
    key: &str,
) -> anyhow::Result<()> {
    granule_bytes(client, satellite, key).await.map(|_| ())
}

/// The newest CONUS CMIP key for `band` on `satellite`, checking this UTC hour and falling back to
/// the previous one (covers the few minutes after an hour rolls over before anything has landed).
async fn latest_key(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now();
    for hours_ago in [0i64, 1] {
        let Some(t) = now.checked_sub_signed(chrono::Duration::hours(hours_ago)) else {
            continue;
        };
        // Keys within one hour are already in lexicographic == chronological order (zero-padded
        // fields), so the last one in the listing is the newest — no need for `key_time` here.
        if let Some(key) = keys_in_hour(client, satellite, sector, band, t).await.pop() {
            return Ok(key);
        }
    }
    anyhow::bail!("no ABI CMIP band {band} objects found for the last two hours")
}

/// The key whose own scan time is closest to `target`, searched across the hour containing
/// `target` plus the hour immediately before and after it — a target a few minutes from an hour
/// boundary can have its nearest neighbor filed under either side.
async fn key_near(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
    target: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<String> {
    let mut keys = Vec::new();
    for hours in [-1i64, 0, 1] {
        let Some(t) = target.checked_add_signed(chrono::Duration::hours(hours)) else {
            continue;
        };
        keys.extend(keys_in_hour(client, satellite, sector, band, t).await);
    }
    keys.into_iter()
        .filter_map(|k| key_time(&k).map(|t| ((t - target).num_seconds().abs(), k)))
        .min_by_key(|(dist, _)| *dist)
        .map(|(_, k)| k)
        .ok_or_else(|| anyhow::anyhow!("no ABI CMIP band {band} objects found near {target}"))
}

/// A granule's bytes, through the object cache (a scan's key carries its start time, so the
/// file never changes).
async fn granule_bytes(
    client: &reqwest::Client,
    satellite: Satellite,
    key: &str,
) -> anyhow::Result<Vec<u8>> {
    let when = key_time(key).unwrap_or_else(chrono::Utc::now);
    let url = format!("{}/{key}", satellite.bucket_at(when));
    crate::objcache::cached(
        &crate::objcache::GOES,
        &url,
        crate::objcache::is_whole_hdf5,
        async {
            let bytes = client
                .get(crate::net::fetch_url(&url))
                .timeout(crate::net::FEED_TIMEOUT)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
                .to_vec();
            crate::stats::net(bytes.len());
            Ok(bytes)
        },
    )
    .await
}

async fn fetch_key(
    client: &reqwest::Client,
    satellite: Satellite,
    key: &str,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let bytes = granule_bytes(client, satellite, key).await?;
    decode(bytes, out_nx, out_ny)
}

/// Cloud top height (km above mean sea level) from the ABI Level 2 ACHA product, CONUS, every five
/// minutes at 10 km (ROADMAP_NEW H6's "satellite cloud-top-height surface when a trustworthy source
/// exists": NOAA's operational product, with its quality flags applied). The newest granule, or
/// with `at` the one nearest that time; resampled onto `out_nx × out_ny`. Pixels without a cloud
/// top (clear sky, or flagged) are `NaN`.
pub async fn fetch_cloud_top_height(
    client: &reqwest::Client,
    satellite: Satellite,
    at: Option<chrono::DateTime<chrono::Utc>>,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    use chrono::{Datelike, Timelike};
    let target = at.unwrap_or_else(chrono::Utc::now);
    let mut keys = Vec::new();
    for hours in [-1i64, 0, 1] {
        let Some(t) = target.checked_add_signed(chrono::Duration::hours(hours)) else {
            continue;
        };
        let prefix = format!(
            "ABI-L2-ACHAC/{:04}/{:03}/{:02}/OR_ABI-L2-ACHAC-M",
            t.year(),
            t.ordinal(),
            t.hour()
        );
        let url = format!(
            "{}/?list-type=2&prefix={prefix}&max-keys=100",
            satellite.bucket_at(t)
        );
        if let Ok(resp) = client.get(crate::net::fetch_url(&url)).send().await {
            if let Ok(xml) = resp.text().await {
                keys.extend(all_keys(&xml));
            }
        }
    }
    let key = keys
        .into_iter()
        .filter_map(|k| key_time(&k).map(|t| ((t - target).num_seconds().abs(), k)))
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
        .ok_or_else(|| anyhow::anyhow!("no ABI cloud-top-height granule near {target}"))?;
    let when = key_time(&key).unwrap_or(target);
    let url = format!("{}/{key}", satellite.bucket_at(when));
    let bytes = crate::objcache::cached(
        &crate::objcache::GOES,
        &url,
        crate::objcache::is_whole_hdf5,
        async {
            let bytes = client
                .get(crate::net::fetch_url(&url))
                .timeout(crate::net::FEED_TIMEOUT)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
                .to_vec();
            crate::stats::net(bytes.len());
            Ok(bytes)
        },
    )
    .await?;
    let mut field = decode_var(bytes, "HT", out_nx, out_ny)?;
    // Metres in the file; kilometres like every other height layer here.
    for v in &mut field.values {
        *v /= 1000.0;
    }
    Ok(field)
}

/// Fetch and decode the latest CONUS CMIP granule for `band` on `satellite`, resampled onto an
/// `out_nx × out_ny` lat/lon grid.
pub async fn fetch_latest_conus(
    client: &reqwest::Client,
    satellite: Satellite,
    band: u8,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    fetch_at(client, satellite, Sector::Conus, band, None, out_nx, out_ny).await
}

/// Fetch and decode a granule for `band` from `sector` on `satellite`: the newest, or with `at`
/// the one whose scan began nearest that time (an archive frame, for a view scrubbed back — the
/// caller checks the result's `time` against its own tolerance). A mesoscale granule's grid spans
/// the box the sector covered at that minute ([`Footprint::of`]).
pub async fn fetch_at(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
    at: Option<chrono::DateTime<chrono::Utc>>,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let key = match at {
        Some(t) => key_near(client, satellite, sector, band, t).await?,
        None => latest_key(client, satellite, sector, band).await?,
    };
    fetch_key(client, satellite, &key, out_nx, out_ny).await
}

/// Band `band` at `target`, interpolated between the scans either side when both lie within
/// `tolerance` of it and decode onto the same grid (1008.md E2): a fixed sector (CONUS, full
/// disk) blends; a mesoscale box that moved between its scans does not. Otherwise, and at an
/// exact scan, the nearest scan as [`fetch_at`] reads it. Returns the field (valid at `target`
/// when blended) and the two scans it lies between.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_blended_at(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    band: u8,
    target: chrono::DateTime<chrono::Utc>,
    tolerance: chrono::Duration,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<(MrmsField, Option<crate::field::TimeBlend>)> {
    let mut keys = Vec::new();
    for hours in [-1i64, 0, 1] {
        let Some(t) = target.checked_add_signed(chrono::Duration::hours(hours)) else {
            continue;
        };
        keys.extend(
            keys_in_hour(client, satellite, sector, band, t)
                .await
                .into_iter()
                .filter_map(|k| Some((key_time(&k)?, k))),
        );
    }
    let before = keys
        .iter()
        .filter(|(t, _)| *t < target && target - *t <= tolerance)
        .max_by_key(|(t, _)| *t);
    let after = keys
        .iter()
        .filter(|(t, _)| *t > target && *t - target <= tolerance)
        .min_by_key(|(t, _)| *t);
    let exact = keys.iter().any(|(t, _)| *t == target);
    if let (false, Some((_, kb)), Some((_, ka))) = (exact, before, after) {
        let (a, b) = futures_util::future::try_join(
            fetch_key(client, satellite, kb, out_nx, out_ny),
            fetch_key(client, satellite, ka, out_nx, out_ny),
        )
        .await?;
        let stamped = |f: MrmsField| {
            let valid = f.time;
            crate::field::Stamped {
                data: f,
                stamp: crate::field::DataStamp {
                    source_id: satellite_label(satellite).into(),
                    product_id: format!("ABI band {band}"),
                    issue_time: None,
                    run_time: None,
                    valid_time: valid,
                    received_time: chrono::Utc::now(),
                    source_latency: None,
                    is_forecast: false,
                    is_derived: false,
                    quality: crate::field::QualitySummary::Unknown,
                    grid: None,
                },
            }
        };
        if let Ok(blended) = crate::field::blend_frames(
            &stamped(a),
            &stamped(b),
            target,
            crate::field::ValueKind::Scalar,
        ) {
            let blend = blended.stamp.grid.as_ref().and_then(|g| g.blend);
            return Ok((blended.data, blend));
        }
    }
    Ok((
        fetch_at(
            client,
            satellite,
            sector,
            band,
            Some(target),
            out_nx,
            out_ny,
        )
        .await?,
        None,
    ))
}

fn satellite_label(satellite: Satellite) -> &'static str {
    match satellite {
        Satellite::East => "GOES-East",
        Satellite::West => "GOES-West",
    }
}

/// Where `sector` is pointed now: the box its newest band 13 granule covers (decoded coarse —
/// only the bounds are wanted; the file is about a megabyte).
pub async fn footprint(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    at: Option<chrono::DateTime<chrono::Utc>>,
) -> anyhow::Result<Footprint> {
    Ok(Footprint::of(
        &fetch_at(client, satellite, sector, 13, at, 32, 32).await?,
    ))
}

/// Fetch several bands from one scan, decoded to the same `out_nx × out_ny` grid (in `bands`'
/// order): the newest scan of the first band, and for each other band its granule nearest that
/// scan's start. A band whose nearest granule is more than `same_scan_secs` away belongs to a
/// different scan, and the whole fetch is refused rather than composing two moments into one
/// picture (ROADMAP_NEW E4's RGB recipes, `crate::goes_rgb`).
#[allow(clippy::too_many_arguments)] // one call site; a params struct buys nothing
pub async fn fetch_same_scan(
    client: &reqwest::Client,
    satellite: Satellite,
    sector: Sector,
    at: Option<chrono::DateTime<chrono::Utc>>,
    bands: &[u8],
    same_scan_secs: i64,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<Vec<MrmsField>> {
    let (&first, rest) = bands
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("no bands asked for"))?;
    // The newest scan, or the one nearest `at` (a view scrubbed back).
    let anchor = match at {
        Some(t) => key_near(client, satellite, sector, first, t).await?,
        None => latest_key(client, satellite, sector, first).await?,
    };
    let when = key_time(&anchor)
        .ok_or_else(|| anyhow::anyhow!("could not parse a scan time from {anchor}"))?;
    let others = futures_util::future::try_join_all(
        rest.iter()
            .map(|&band| key_near(client, satellite, sector, band, when)),
    )
    .await?;
    let mut keys = vec![anchor];
    for (band, key) in rest.iter().zip(others) {
        let t = key_time(&key).ok_or_else(|| anyhow::anyhow!("no scan time in {key}"))?;
        anyhow::ensure!(
            same_scan(when, t, same_scan_secs),
            "band {band} has no granule from the {when} scan (nearest is {t})"
        );
        keys.push(key);
    }
    futures_util::future::try_join_all(
        keys.iter()
            .map(|k| fetch_key(client, satellite, k, out_nx, out_ny)),
    )
    .await
}

/// Whether two granule start times belong to one scan.
fn same_scan(
    a: chrono::DateTime<chrono::Utc>,
    b: chrono::DateTime<chrono::Utc>,
    secs: i64,
) -> bool {
    (a - b).num_seconds().abs() <= secs
}

/// Fetch two bands from the same satellite and subtract their brightness temperatures cell by
/// cell (`band_a` minus `band_b`), for the classic channel-difference analysis techniques
/// (ROADMAP_NEW E6, e.g. the split-window dust/ash product: Band 15 minus Band 13). The two
/// fetches run concurrently — they're independent S3 objects — then [`decode`] is asked for the
/// same `out_nx`/`out_ny` for both, which means their output grids are always the same shape
/// (that parameter is passed straight through, never derived from a granule's own content), so
/// the subtraction is a plain cell-by-cell op with no resampling step: the two bands see the same
/// fixed sector from the same instrument, so their real geographic extents already agree to well
/// under a pixel at CONUS resolution. This deliberately does *not* reuse
/// `hookecho::fielddiff::diff` (built for model comparison, where an exact valid-time match is
/// meaningful and enforced) — two ABI bands from the same scan have close-but-not-bit-identical
/// timestamps by design, so that function's strict `a.time != b.time → None` would always reject
/// a genuine same-scan pair.
pub async fn fetch_latest_conus_diff(
    client: &reqwest::Client,
    satellite: Satellite,
    band_a: u8,
    band_b: u8,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let (a, b) = futures_util::future::try_join(
        fetch_latest_conus(client, satellite, band_a, out_nx, out_ny),
        fetch_latest_conus(client, satellite, band_b, out_nx, out_ny),
    )
    .await?;
    diff_fields(&a, &b)
}

/// Fetch the same band `lookback_minutes` apart and subtract (the earlier granule minus the
/// latest), for a brightness-temperature *trend* rather than a snapshot — ROADMAP_NEW E6's
/// "cooling-rate/time-change product". Ordered so a positive value is cooling (cloud-top
/// temperature dropping — the direction a rapidly intensifying updraft's overshooting top would
/// show, which a single frame can't) and negative is warming, the same "positive means something
/// worth looking at" convention `GoesDustDiff`/`GoesColdTop` already use for their own ramps.
/// `lookback_minutes` need not land on an exact scan — [`key_near`] finds whichever real granule
/// is closest, so a missed scan degrades to a slightly different actual interval rather than an
/// error. Reuses [`diff_fields`] (the same shape-checked cell-by-cell subtraction
/// `fetch_latest_conus_diff` uses above), just on two *times* of one band instead of two bands of
/// one time — and does not reuse `hookecho::fielddiff::diff` for the identical reason that
/// function's doc comment above already gives for the band-difference case.
pub async fn fetch_cooling_rate(
    client: &reqwest::Client,
    satellite: Satellite,
    band: u8,
    lookback_minutes: i64,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let latest = latest_key(client, satellite, Sector::Conus, band).await?;
    let latest_time = key_time(&latest)
        .ok_or_else(|| anyhow::anyhow!("could not parse a scan time from {latest}"))?;
    let target = latest_time - chrono::Duration::minutes(lookback_minutes);
    let earlier = key_near(client, satellite, Sector::Conus, band, target).await?;
    let (now_field, past_field) = futures_util::future::try_join(
        fetch_key(client, satellite, &latest, out_nx, out_ny),
        fetch_key(client, satellite, &earlier, out_nx, out_ny),
    )
    .await?;
    diff_fields(&past_field, &now_field)
}

/// `a - b`, cell by cell, keeping `a`'s own bounds/time. Both grids must be the same shape — true
/// for any two [`decode`] outputs requested at the same `out_nx`/`out_ny`, which is the only way
/// this is called; a shape mismatch is a caller bug, not a runtime condition to recover from
/// gracefully, so it's a named error rather than a silent truncation or panic.
fn diff_fields(a: &MrmsField, b: &MrmsField) -> anyhow::Result<MrmsField> {
    anyhow::ensure!(
        a.nx == b.nx && a.ny == b.ny,
        "channel-difference grids have different shapes: {}x{} vs {}x{}",
        a.nx,
        a.ny,
        b.nx,
        b.ny
    );
    let values = a
        .values
        .iter()
        .zip(&b.values)
        .map(|(&x, &y)| x - y) // NaN propagates through arithmetic — either side missing, so is this
        .collect();
    Ok(MrmsField {
        values,
        nx: a.nx,
        ny: a.ny,
        lon_west: a.lon_west,
        lon_east: a.lon_east,
        lat_north: a.lat_north,
        lat_south: a.lat_south,
        time: a.time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_satellite_is_the_one_in_place_at_the_time() {
        use chrono::TimeZone;
        let t = |y, m, d| chrono::Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap();
        assert!(Satellite::East.bucket_at(t(2021, 6, 1)).contains("goes16"));
        assert!(Satellite::East.bucket_at(t(2026, 9, 1)).contains("goes19"));
        assert!(Satellite::West.bucket_at(t(2022, 6, 1)).contains("goes17"));
        assert!(Satellite::West.bucket_at(t(2024, 6, 1)).contains("goes18"));
    }

    /// An archive case from 2021 (GOES-16, and a pre-2019-style listing check), live.
    /// `cargo test -p wxdata goes_archive_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn goes_archive_live() {
        use chrono::TimeZone;
        let at = chrono::Utc
            .with_ymd_and_hms(2021, 12, 11, 3, 30, 0)
            .unwrap();
        let f = fetch_at(
            &reqwest::Client::new(),
            Satellite::East,
            Sector::Conus,
            13,
            Some(at),
            300,
            200,
        )
        .await
        .unwrap();
        println!("{} ({}x{})", f.time, f.nx, f.ny);
        assert!((f.time - at).num_minutes().abs() <= 10);
    }

    /// The last hour of Mesoscale 1 band 13 scans, live: about sixty one-minute frames, a second
    /// listing served from the cache, and one granule prefetched into the object cache.
    /// `cargo test -p wxdata meso_loop_listing_live -- --ignored --nocapture`
    /// Live (network): GOES-East CONUS band 13 two hours ago, a third of the way between two
    /// consecutive scans, blended (`fetch_blended_at`) and checked cell by cell against the two
    /// scans themselves. Writes `target/parity-review/m5.4/goes-blend-live.txt`.
    #[tokio::test]
    #[ignore = "network"]
    async fn goes_blend_live() {
        let client = reqwest::Client::new();
        let around = chrono::Utc::now() - chrono::Duration::hours(2);
        let mut keys: Vec<(chrono::DateTime<chrono::Utc>, String)> =
            keys_in_hour(&client, Satellite::East, Sector::Conus, 13, around)
                .await
                .into_iter()
                .filter_map(|k| Some((key_time(&k)?, k)))
                .collect();
        keys.sort();
        let pair = keys.windows(2).next().expect("two scans in the hour");
        let (t0, t1) = (pair[0].0, pair[1].0);
        let target = t0 + (t1 - t0) / 3;
        let (nx, ny) = (600, 360);
        let (blended, blend) = fetch_blended_at(
            &client,
            Satellite::East,
            Sector::Conus,
            13,
            target,
            chrono::Duration::minutes(15),
            nx,
            ny,
        )
        .await
        .unwrap();
        let blend = blend.expect("blended between the two scans");
        assert_eq!(blended.time, target);
        let a = fetch_key(&client, Satellite::East, &pair[0].1, nx, ny)
            .await
            .unwrap();
        let b = fetch_key(&client, Satellite::East, &pair[1].1, nx, ny)
            .await
            .unwrap();
        // The blend is weighted by the scans' decoded valid times, which follow the scan-start
        // time in the file name.
        assert_eq!((blend.before, blend.after), (a.time, b.time));
        let w = blend.weight_after as f32;
        let (mut both, mut worst, mut changed) = (0usize, 0f32, 0usize);
        for ((x, y), m) in a.values.iter().zip(&b.values).zip(&blended.values) {
            if x.is_finite() && y.is_finite() {
                both += 1;
                worst = worst.max((m - (x + (y - x) * w)).abs());
                if (x - y).abs() > 0.5 {
                    changed += 1;
                }
            } else {
                assert!(m.is_nan());
            }
        }
        let report = format!(
            "GOES-East CONUS band 13 (brightness temperature, K)\nscans starting {t0} and {t1}, valid {} and {}\nblended at {target} (weight_after {:.3}, between the valid times)\ngrid {nx}x{ny}; cells finite in both scans {both}; cells that changed by more than 0.5 K between the scans {changed}\nworst |blend - linear interpolation| {worst:e} K\n",
            a.time,
            b.time,
            blend.weight_after
        );
        print!("{report}");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m5.4");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("goes-blend-live.txt"), report).unwrap();
        assert!(both > 100_000 && worst < 1e-3);
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn meso_loop_listing_live() {
        let client = reqwest::Client::new();
        let to = chrono::Utc::now();
        let from = to - chrono::Duration::minutes(60);
        let t0 = std::time::Instant::now();
        let frames = scan_frames(&client, Satellite::East, Sector::Meso1, 13, from, to)
            .await
            .unwrap();
        let cold = t0.elapsed();
        let t1 = std::time::Instant::now();
        let again = scan_frames(&client, Satellite::East, Sector::Meso1, 13, from, to)
            .await
            .unwrap();
        let warm = t1.elapsed();
        let gaps = scan_gaps(&frames, Sector::Meso1.cadence_secs());
        println!(
            "{} frames {} .. {}, {} gaps ({} missing); listed in {cold:?}, again in {warm:?}",
            frames.len(),
            frames.first().map(|f| f.start).unwrap(),
            frames.last().map(|f| f.start).unwrap(),
            gaps.len(),
            gaps.iter().map(|g| g.missing).sum::<u32>()
        );
        assert!(frames.len() >= 45, "one-minute scans over an hour");
        assert_eq!(frames, again);
        let t2 = std::time::Instant::now();
        prefetch_key(&client, Satellite::East, &frames.last().unwrap().key)
            .await
            .unwrap();
        println!("prefetched the newest granule in {:?}", t2.elapsed());
    }

    fn goes_east_projection() -> Projection {
        // GOES-19's real goes_imager_projection values (WGS84-flavoured GRS80 ellipsoid).
        Projection {
            req: 6_378_137.0,
            rpol: 6_356_752.314_14,
            h: 35_786_023.0 + 6_378_137.0,
            lon_origin: (-75.0f64).to_radians(),
        }
    }

    /// Looking straight down the satellite's own line of sight (`x = y = 0`) has to read back the
    /// projection origin exactly — the one point on the formula simple enough to check by hand.
    #[test]
    fn the_sub_satellite_point_reads_back_the_projection_origin() {
        let proj = goes_east_projection();
        let (lon, lat) = proj.scan_to_lonlat(0.0, 0.0).expect("on the disk");
        assert!((lon - (-75.0)).abs() < 1e-9, "lon {lon}");
        assert!(lat.abs() < 1e-9, "lat {lat}");
    }

    /// Real key names from a live `noaa-goes19` listing (see this module's own `key_time` doc
    /// comment) — not fabricated, so the parser is checked against the actual NOAA naming
    /// convention rather than a guess at it.
    const REAL_KEY: &str =
        "ABI-L2-CMIPC/2026/262/18/OR_ABI-L2-CMIPC-M6C13_G19_s20262621801173_e20262621803558_c20262621804022.nc";

    #[test]
    fn key_time_parses_a_real_scan_start_timestamp() {
        let t = key_time(REAL_KEY).expect("real key parses");
        assert_eq!(t.to_string(), "2026-09-19 18:01:17 UTC");
    }

    #[test]
    fn key_time_rejects_garbage_without_panicking() {
        assert_eq!(key_time("not a key at all"), None);
        assert_eq!(
            key_time("OR_ABI-L2-CMIPC-M6C13_G19_sNOTDIGITS_e...nc"),
            None
        );
        // A truncated `_s` field (fewer than the 14 digits a real one always has) must not panic
        // on the fixed-width slice indexing `key_time` does internally.
        assert_eq!(key_time("OR_..._s202626_e...nc"), None);
    }

    #[test]
    fn all_keys_extracts_every_key_from_a_listing_in_order() {
        let xml = format!(
            "<ListBucketResult><Contents><Key>{a}</Key></Contents>\
             <Contents><Key>{b}</Key></Contents></ListBucketResult>",
            a = REAL_KEY,
            b = "ABI-L2-CMIPC/2026/262/18/OR_ABI-L2-CMIPC-M6C13_G19_s20262621806173_e20262621808558_c20262621809040.nc",
        );
        let keys = all_keys(&xml);
        assert_eq!(keys.len(), 2);
        assert!(keys[0].ends_with("s20262621801173_e20262621803558_c20262621804022.nc"));
        assert!(keys[1].ends_with("s20262621806173_e20262621808558_c20262621809040.nc"));
    }

    /// A mesoscale key in the live naming, for scan start `hhmmss` on 2026 day 262.
    fn meso_key(band: u8, hhmmss: &str) -> String {
        format!(
            "ABI-L2-CMIPM/2026/262/18/OR_ABI-L2-CMIPM1-M6C{band:02}_G19_s2026262{hhmmss}2_e2026262{hhmmss}9_c2026262{hhmmss}9.nc"
        )
    }

    #[test]
    fn a_listing_becomes_one_bands_scans_in_order_with_its_gaps() {
        // Band 13 every minute 18:00-18:06 except 18:03 and 18:04, listed out of order, with a
        // band 2 key and a duplicate delivery mixed in.
        let keys: Vec<String> = ["180517", "180017", "180117", "180217", "180617"]
            .iter()
            .map(|t| meso_key(13, t))
            .chain([meso_key(2, "180117"), meso_key(13, "180017")])
            .collect();
        let frames = scan_frames_from_keys(&keys, 13);
        let mins: Vec<u32> = frames
            .iter()
            .map(|f| chrono::Timelike::minute(&f.start))
            .collect();
        assert_eq!(mins, [0, 1, 2, 5, 6], "band 13 only, sorted, one per scan");
        assert!(frames.iter().all(|f| f.key.contains("C13_G")));
        let gaps = scan_gaps(&frames, Sector::Meso1.cadence_secs());
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert_eq!(chrono::Timelike::minute(&gaps[0].after), 2);
        assert_eq!(chrono::Timelike::minute(&gaps[0].before), 5);
        assert_eq!(gaps[0].missing, 2, "18:03 and 18:04");
        // A CONUS cadence over the same frames sees no gap a minute apart could make.
        assert!(scan_gaps(&frames[..3], Sector::Conus.cadence_secs()).is_empty());
        // Ordinary jitter (a scan 70 s after the last) is not a gap.
        let jitter = vec![
            ScanFrame {
                start: frames[0].start,
                key: String::new(),
            },
            ScanFrame {
                start: frames[0].start + chrono::Duration::seconds(70),
                key: String::new(),
            },
        ];
        assert!(scan_gaps(&jitter, 60).is_empty());
    }

    #[test]
    fn a_finished_hours_listing_is_kept_and_an_open_ones_is_not() {
        use chrono::TimeZone;
        let hour = chrono::Utc.with_ymd_and_hms(2026, 9, 19, 18, 0, 0).unwrap();
        let mins = |m| hour + chrono::Duration::minutes(m);
        // Read long after the hour settled: good forever.
        assert!(listing_fresh(hour, mins(90), mins(10_000)));
        // Read while the hour was filling: good for seconds only.
        assert!(listing_fresh(
            hour,
            mins(30),
            mins(30) + chrono::Duration::seconds(5)
        ));
        assert!(!listing_fresh(hour, mins(30), mins(31)));
        // Read just after the hour ended, before late files settle: still reread.
        assert!(!listing_fresh(hour, mins(65), mins(67)));
    }

    #[test]
    fn each_sector_names_its_own_files_and_cadence() {
        // As the live bucket files them (checked against a listing).
        assert_eq!(
            (Sector::Meso1.product(), Sector::Meso1.file_product()),
            ("ABI-L2-CMIPM", "ABI-L2-CMIPM1")
        );
        assert_eq!(Sector::Meso2.file_product(), "ABI-L2-CMIPM2");
        assert_eq!(Sector::Conus.product(), Sector::Conus.file_product());
        assert_eq!(Sector::Meso1.cadence_secs(), 60);
        assert_eq!(Sector::Conus.cadence_secs(), 300);
        assert!(Sector::Meso2.is_meso() && !Sector::Conus.is_meso());
        let key = "ABI-L2-CMIPM/2026/270/17/OR_ABI-L2-CMIPM1-M6C13_G19_s20262701701247_e20262701701316_c20262701701368.nc";
        assert_eq!(
            key_time(key).map(|t| t.to_rfc3339()),
            Some("2026-09-27T17:01:24+00:00".to_string()),
            "a mesoscale key's scan time parses like a CONUS one"
        );
    }

    #[test]
    fn a_footprint_is_the_box_its_granule_covers() {
        let f = MrmsField {
            values: vec![0.0; 4],
            nx: 2,
            ny: 2,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 40.0,
            lat_south: 32.0,
            time: chrono::DateTime::UNIX_EPOCH,
        };
        let fp = Footprint::of(&f);
        assert!(fp.contains(-95.0, 35.0));
        assert!(!fp.contains(-101.0, 35.0) && !fp.contains(-95.0, 41.0));
    }

    #[test]
    fn all_keys_on_a_listing_with_no_contents_is_empty_not_an_error() {
        assert!(all_keys("<ListBucketResult></ListBucketResult>").is_empty());
    }

    /// A scan angle far enough off-axis to miss the Earth's disk (well past the limb) must not
    /// come back as a bogus lon/lat instead of `None`.
    #[test]
    fn a_scan_angle_off_the_disk_is_none() {
        let proj = goes_east_projection();
        assert!(
            proj.scan_to_lonlat(1.0, 1.0).is_none(),
            "1 radian is way off-disk"
        );
    }

    /// Small scan angles east/north of straight-down move lon/lat in the expected direction —
    /// catches a sign error the sub-satellite-point check alone can't (it's all zeros).
    #[test]
    fn small_angles_move_lonlat_the_right_way() {
        let proj = goes_east_projection();
        let (lon0, lat0) = proj.scan_to_lonlat(0.0, 0.0).unwrap();
        // +x scans east of the sub-satellite meridian.
        let (lon_e, _) = proj.scan_to_lonlat(0.01, 0.0).unwrap();
        assert!(lon_e > lon0, "east scan angle should increase longitude");
        // +y scans north.
        let (_, lat_n) = proj.scan_to_lonlat(0.0, 0.01).unwrap();
        assert!(lat_n > lat0, "north scan angle should increase latitude");
    }

    #[test]
    fn from_attrs_reads_the_real_attribute_set() {
        use std::collections::HashMap;
        let mut attrs = HashMap::new();
        attrs.insert(
            "semi_major_axis".to_string(),
            hdf5lite::Value::Num(6_378_137.0),
        );
        attrs.insert(
            "semi_minor_axis".to_string(),
            hdf5lite::Value::Num(6_356_752.314_14),
        );
        attrs.insert(
            "perspective_point_height".to_string(),
            hdf5lite::Value::Num(35_786_023.0),
        );
        attrs.insert(
            "longitude_of_projection_origin".to_string(),
            hdf5lite::Value::Num(-75.0),
        );
        let proj = Projection::from_attrs(&attrs).expect("all required attrs present");
        assert!((proj.lon_origin.to_degrees() - (-75.0)).abs() < 1e-9);
        assert!((proj.h - (35_786_023.0 + 6_378_137.0)).abs() < 1e-6);
    }

    #[test]
    fn from_attrs_is_none_when_a_required_attribute_is_missing() {
        use std::collections::HashMap;
        let attrs = HashMap::from([(
            "semi_major_axis".to_string(),
            hdf5lite::Value::Num(6_378_137.0),
        )]);
        assert!(Projection::from_attrs(&attrs).is_none());
    }

    fn synthetic_field(values: Vec<f32>, nx: usize, ny: usize) -> MrmsField {
        MrmsField {
            values,
            nx,
            ny,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 40.0,
            lat_south: 30.0,
            time: chrono::Utc::now(),
        }
    }

    #[test]
    fn diff_fields_subtracts_cell_by_cell_and_keeps_as_bounds() {
        let a = synthetic_field(vec![250.0, 260.0, 270.0, 280.0], 2, 2);
        let b = synthetic_field(vec![248.0, 262.0, 268.0, 280.0], 2, 2);
        let d = diff_fields(&a, &b).unwrap();
        assert_eq!(d.values, vec![2.0, -2.0, 2.0, 0.0]);
        assert_eq!(d.lon_west, a.lon_west);
        assert_eq!(d.time, a.time);
    }

    #[test]
    fn diff_fields_propagates_missing_data_as_nan() {
        let a = synthetic_field(vec![250.0, f32::NAN], 2, 1);
        let b = synthetic_field(vec![f32::NAN, 260.0], 2, 1);
        let d = diff_fields(&a, &b).unwrap();
        assert!(
            d.values[0].is_nan(),
            "a missing input must not fabricate a difference"
        );
        assert!(d.values[1].is_nan());
    }

    #[test]
    fn diff_fields_rejects_mismatched_shapes() {
        let a = synthetic_field(vec![0.0; 4], 2, 2);
        let b = synthetic_field(vec![0.0; 6], 3, 2);
        assert!(diff_fields(&a, &b).is_err());
    }

    #[test]
    fn mask_by_dqf_clears_every_non_zero_flag() {
        let mut cmi = vec![210.0, 211.0, 212.0, 213.0];
        let dqf = vec![0.0, 1.0, 2.0, 3.0];
        mask_by_dqf(&mut cmi, &dqf);
        assert_eq!(cmi[0], 210.0, "DQF 0 (good) must survive untouched");
        assert!(cmi[1].is_nan(), "DQF 1 (conditionally usable) is masked");
        assert!(cmi[2].is_nan(), "DQF 2 (out of range) is masked");
        assert!(cmi[3].is_nan(), "DQF 3 (no value) is masked");
    }

    #[test]
    fn mask_by_dqf_leaves_cmi_untouched_on_a_shape_mismatch() {
        let mut cmi = vec![210.0, 211.0];
        let dqf = vec![1.0]; // wrong length — a real granule never does this
        mask_by_dqf(&mut cmi, &dqf);
        assert_eq!(
            cmi,
            vec![210.0, 211.0],
            "a shape mismatch must not partially mask or panic"
        );
    }

    #[test]
    fn mask_by_dqf_treats_a_nan_flag_as_not_good() {
        // A DQF band's own fill value (unexpected for a real granule, but this reader doesn't
        // assume one can't appear) reads back as NaN through the same read_f64 path CMI uses —
        // `NaN != 0.0` is true in IEEE 754, so this already masks correctly, but it's worth
        // pinning down explicitly rather than relying on that being obvious from the comparison.
        let mut cmi = vec![210.0];
        let dqf = vec![f64::NAN];
        mask_by_dqf(&mut cmi, &dqf);
        assert!(cmi[0].is_nan());
    }

    /// End-to-end against a real granule: the same GOES-19 mesoscale Band 13 fixture
    /// `hdf5lite`'s own regression test uses (its `DIMENSION_LIST`-overflow fixture doubles as a
    /// real decode target here). This mesoscale sector sat over the western Atlantic/Caribbean
    /// area at capture time, so the check is deliberately loose — "somewhere plausible on Earth
    /// near the Americas", not an exact scene match.
    #[test]
    fn decodes_a_real_granule_to_a_plausible_geographic_extent() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hdf5lite/tests/data/regression/abi_cmip_m6c13.nc");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let field = decode(bytes, 100, 100).expect("decode");
        assert_eq!(field.nx, 100);
        assert_eq!(field.ny, 100);
        assert!(
            (-100.0..-30.0).contains(&field.lon_west) && (-100.0..-30.0).contains(&field.lon_east),
            "lon {}..{} not in the Americas",
            field.lon_west,
            field.lon_east
        );
        assert!(
            (-10.0..50.0).contains(&field.lat_south) && (-10.0..50.0).contains(&field.lat_north),
            "lat {}..{} not plausible for GOES-East",
            field.lat_south,
            field.lat_north
        );
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(
            finite > field.values.len() / 2,
            "too many unfilled cells: {finite}/{}",
            field.values.len()
        );
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!(
                (150.0..350.0).contains(&v),
                "implausible brightness temp {v} K"
            );
        }
    }

    /// Confirms `decode` actually reads the real fixture's own `DQF` band and hands it to
    /// `mask_by_dqf` with matching shape, rather than that function's own (synthetic-data) unit
    /// tests being the only thing exercising this path. `mask_by_dqf` itself is already proven
    /// correct against synthetic data above; what a real granule adds is confirming the band
    /// exists, decodes to the expected length, and — checked here directly, bypassing `decode`'s
    /// reprojection — is applied in a way consistent with this fixture's own content, whatever
    /// that happens to be (this particular fixture turns out to be an entirely clean scene, every
    /// pixel DQF 0 — a real, useful finding in its own right rather than a reason to force a
    /// stronger assertion this fixture can't actually back up).
    #[test]
    fn decode_reads_the_fixtures_own_dqf_band_at_matching_shape() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hdf5lite/tests/data/regression/abi_cmip_m6c13.nc");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let f = hdf5lite::File::open(bytes.clone()).unwrap();
        let cmi = f.read_f64("CMI").unwrap();
        let dqf = f
            .read_f64("DQF")
            .expect("a real CMIP granule always carries DQF");
        assert_eq!(
            dqf.len(),
            cmi.len(),
            "DQF and CMI must be the same shape to mask by"
        );

        let source_masked = dqf.iter().filter(|&&flag| flag != 0.0).count();
        if source_masked == 0 {
            // This fixture happens to be a fully clean scene — nothing for masking to remove, so
            // the strongest available check is that decode() still succeeds and DQF-reading
            // didn't somehow corrupt an all-good scene into a mostly-empty one.
            let field = decode(bytes, 100, 100).expect("decode");
            let output_finite = field.values.iter().filter(|v| v.is_finite()).count();
            assert!(
                output_finite > field.values.len() / 2,
                "an all-DQF-0 scene must not come out mostly empty after masking"
            );
            return;
        }
        // The fixture does contain masked pixels (a future fixture swap, or this one turning out
        // to have some at a resolution this test didn't check before) — verify decode()'s output
        // fraction actually drops relative to an unmasked reading, the direction that matters.
        let source_finite_unmasked = cmi.iter().filter(|v| v.is_finite()).count();
        let field = decode(bytes, 200, 200).expect("decode");
        let output_finite = field.values.iter().filter(|v| v.is_finite()).count();
        let output_fraction = output_finite as f64 / field.values.len() as f64;
        let source_fraction_unmasked = source_finite_unmasked as f64 / cmi.len() as f64;
        assert!(
            output_fraction < source_fraction_unmasked,
            "masking {source_masked} not-good source pixels did not measurably reduce the \
             decoded output's own finite fraction ({output_fraction:.3} vs unmasked source \
             {source_fraction_unmasked:.3}) — DQF doesn't appear to be taking effect"
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_the_live_conus_clean_ir() {
        let client = reqwest::Client::new();
        let field = fetch_latest_conus(&client, Satellite::East, 13, 200, 150)
            .await
            .expect("fetch_latest_conus");
        eprintln!(
            "CONUS band 13: {}x{} lon {:.1}..{:.1} lat {:.1}..{:.1} at {}",
            field.nx,
            field.ny,
            field.lon_west,
            field.lon_east,
            field.lat_south,
            field.lat_north,
            field.time
        );
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 2);
    }

    /// Band 2 (visible) is reflectance, not brightness temperature — a different physical
    /// quantity than every other band this module reads — so it gets its own live check rather
    /// than trusting that `fetch_latest_conus` being band-generic means every band actually
    /// decodes. At night this is mostly a near-zero field, which is real data, not a fetch
    /// failure, so this only asserts the fetch/decode/regrid pipeline itself succeeds.
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_the_live_conus_visible() {
        let client = reqwest::Client::new();
        let field = fetch_latest_conus(&client, Satellite::East, 2, 200, 150)
            .await
            .expect("fetch_latest_conus");
        eprintln!(
            "CONUS band 2: {}x{} lon {:.1}..{:.1} lat {:.1}..{:.1} at {}",
            field.nx,
            field.ny,
            field.lon_west,
            field.lon_east,
            field.lat_south,
            field.lat_north,
            field.time
        );
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 2);
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!(
                (0.0..2.0).contains(&v),
                "implausible reflectance factor {v}"
            );
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_the_live_conus_water_vapor() {
        let client = reqwest::Client::new();
        let field = fetch_latest_conus(&client, Satellite::East, 8, 200, 150)
            .await
            .expect("fetch_latest_conus");
        eprintln!(
            "CONUS band 8: {}x{} lon {:.1}..{:.1} lat {:.1}..{:.1} at {}",
            field.nx,
            field.ny,
            field.lon_west,
            field.lon_east,
            field.lat_south,
            field.lat_north,
            field.time
        );
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 2);
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!(
                (150.0..300.0).contains(&v),
                "implausible brightness temp {v} K"
            );
        }
    }

    /// ROADMAP_NEW E6's cooling-rate/time-change product, against the real bucket: confirms
    /// `key_near` actually finds a real granule ~15 minutes before the latest one (not just that
    /// `key_time`'s parser works against one hand-picked key above), and that the field this
    /// produces is dominated by small, stable-scene swings the way a real 15-minute BT trend
    /// should be. Deliberately does *not* bound the single largest swing: a genuine, rapidly
    /// forming storm cell somewhere in the whole CONUS domain can legitimately swing 60-100+ K at
    /// one pixel in 15 minutes (clear ground to a cold overshooting top) — exactly the signal this
    /// product exists to surface, not a bug, and a first real run of this test hit exactly that
    /// (an 82 K max during active September convection) before this was corrected to check the
    /// bulk of the scene instead of its most extreme pixel.
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_the_live_cooling_rate() {
        let client = reqwest::Client::new();
        let field = fetch_cooling_rate(&client, Satellite::East, 13, 15, 200, 150)
            .await
            .expect("fetch_cooling_rate");
        eprintln!(
            "cooling rate band 13, 15 min: {}x{} at {}",
            field.nx, field.ny, field.time
        );
        let mut finite: Vec<f32> = field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        assert!(
            finite.len() > field.values.len() / 2,
            "too many unfilled cells: {}/{}",
            finite.len(),
            field.values.len()
        );
        finite.sort_by(|a, b| a.abs().total_cmp(&b.abs()));
        let median_abs = finite[finite.len() / 2].abs();
        let p95_abs = finite[finite.len() * 95 / 100].abs();
        eprintln!("|delta| median {median_abs:.2} K, p95 {p95_abs:.2} K");
        assert!(
            median_abs < 5.0,
            "most of a real CONUS scene should barely change in 15 minutes, got median {median_abs} K"
        );
        assert!(
            p95_abs < 30.0,
            "even the more active 5% of the scene swinging {p95_abs} K in 15 minutes is implausible"
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn finds_where_mesoscale_sector_1_is_pointed() {
        let client = reqwest::Client::new();
        let fp = footprint(&client, Satellite::East, Sector::Meso1, None)
            .await
            .unwrap();
        eprintln!("meso 1: {fp:?}");
        let (w, h) = (fp.lon_east - fp.lon_west, fp.lat_north - fp.lat_south);
        assert!(
            (3.0..30.0).contains(&w) && (3.0..30.0).contains(&h),
            "{w} x {h} degrees"
        );
        assert!(
            (chrono::Utc::now() - fp.time).num_minutes() < 30,
            "a fresh minute"
        );
    }

    /// An archive frame: the granule nearest an hour ago, not the newest.
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_the_mesoscale_frame_nearest_a_past_time() {
        let client = reqwest::Client::new();
        let target = chrono::Utc::now() - chrono::Duration::minutes(63);
        let f = fetch_at(
            &client,
            Satellite::East,
            Sector::Meso1,
            13,
            Some(target),
            60,
            60,
        )
        .await
        .unwrap();
        let off = (f.time - target).num_seconds().abs();
        eprintln!("asked {target}, got {} ({off} s off)", f.time);
        assert!(off <= 60, "a mesoscale sector scans every minute");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_a_real_cloud_top_height_field() {
        let f = fetch_cloud_top_height(&reqwest::Client::new(), Satellite::East, None, 600, 350)
            .await
            .unwrap();
        let tops: Vec<f32> = f.values.iter().copied().filter(|v| v.is_finite()).collect();
        let max = tops.iter().copied().fold(0.0f32, f32::max);
        eprintln!(
            "ACHA at {}: {} cloudy cells of {}, highest {max:.1} km, lon {:.1}..{:.1}",
            f.time,
            tops.len(),
            f.values.len(),
            f.lon_west,
            f.lon_east
        );
        assert!(!tops.is_empty());
        assert!(
            tops.iter().all(|&v| (-1.0..25.0).contains(&v)),
            "km, not metres"
        );
    }
}
