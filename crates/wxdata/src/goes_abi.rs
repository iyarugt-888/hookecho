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
    fn bucket(self) -> &'static str {
        match self {
            Satellite::East => "https://noaa-goes19.s3.amazonaws.com",
            Satellite::West => "https://noaa-goes18.s3.amazonaws.com",
        }
    }
}

/// CONUS: available every ~5 minutes from both satellites and covers the whole area this app's
/// other overlays care about, unlike a mesoscale sector (repositionable, not always pointed
/// anywhere useful) or a full disk (10-minute cadence, mostly ocean for a US-focused app).
const PRODUCT: &str = "ABI-L2-CMIPC";

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
    let f = hdf5lite::File::open(bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    let proj_attrs = f
        .attributes("goes_imager_projection")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let proj = Projection::from_attrs(&proj_attrs)
        .ok_or_else(|| anyhow::anyhow!("goes_imager_projection missing required attributes"))?;

    let x = f.read_f64("x").map_err(|e| anyhow::anyhow!("x: {e}"))?;
    let y = f.read_f64("y").map_err(|e| anyhow::anyhow!("y: {e}"))?;
    let mut cmi = f.read_f64("CMI").map_err(|e| anyhow::anyhow!("CMI: {e}"))?;
    let (nx, ny) = (x.len(), y.len());
    anyhow::ensure!(
        cmi.len() == nx * ny,
        "CMI is {} values, expected {nx}x{ny}={}",
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
fn key_time(key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
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

/// Every CMIP key for `band` on `satellite` in the UTC hour `t` falls in — one listing request,
/// silently empty (not an error) for an hour nothing has landed in yet, or ever will have.
async fn keys_in_hour(
    client: &reqwest::Client,
    satellite: Satellite,
    band: u8,
    t: chrono::DateTime<chrono::Utc>,
) -> Vec<String> {
    use chrono::{Datelike, Timelike};
    let prefix = format!(
        "{PRODUCT}/{:04}/{:03}/{:02}/OR_{PRODUCT}-M6C{band:02}_",
        t.year(),
        t.ordinal(),
        t.hour()
    );
    let url = format!(
        "{}/?list-type=2&prefix={prefix}&max-keys=50",
        satellite.bucket()
    );
    let Ok(resp) = client.get(crate::net::fetch_url(&url)).send().await else {
        return Vec::new();
    };
    let Ok(xml) = resp.text().await else {
        return Vec::new();
    };
    all_keys(&xml)
}

/// The newest CONUS CMIP key for `band` on `satellite`, checking this UTC hour and falling back to
/// the previous one (covers the few minutes after an hour rolls over before anything has landed).
async fn latest_key(
    client: &reqwest::Client,
    satellite: Satellite,
    band: u8,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now();
    for hours_ago in [0i64, 1] {
        let Some(t) = now.checked_sub_signed(chrono::Duration::hours(hours_ago)) else {
            continue;
        };
        // Keys within one hour are already in lexicographic == chronological order (zero-padded
        // fields), so the last one in the listing is the newest — no need for `key_time` here.
        if let Some(key) = keys_in_hour(client, satellite, band, t).await.pop() {
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
    band: u8,
    target: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<String> {
    let mut keys = Vec::new();
    for hours in [-1i64, 0, 1] {
        let Some(t) = target.checked_add_signed(chrono::Duration::hours(hours)) else {
            continue;
        };
        keys.extend(keys_in_hour(client, satellite, band, t).await);
    }
    keys.into_iter()
        .filter_map(|k| key_time(&k).map(|t| ((t - target).num_seconds().abs(), k)))
        .min_by_key(|(dist, _)| *dist)
        .map(|(_, k)| k)
        .ok_or_else(|| anyhow::anyhow!("no ABI CMIP band {band} objects found near {target}"))
}

async fn fetch_key(
    client: &reqwest::Client,
    satellite: Satellite,
    key: &str,
    out_nx: usize,
    out_ny: usize,
) -> anyhow::Result<MrmsField> {
    let url = format!("{}/{key}", satellite.bucket());
    // A scan's key carries its start time, so the file never changes: keep it (`objcache`).
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
    decode(bytes, out_nx, out_ny)
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
    let key = latest_key(client, satellite, band).await?;
    fetch_key(client, satellite, &key, out_nx, out_ny).await
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
    let latest = latest_key(client, satellite, band).await?;
    let latest_time = key_time(&latest)
        .ok_or_else(|| anyhow::anyhow!("could not parse a scan time from {latest}"))?;
    let target = latest_time - chrono::Duration::minutes(lookback_minutes);
    let earlier = key_near(client, satellite, band, target).await?;
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
}
