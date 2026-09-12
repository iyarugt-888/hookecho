//! NDFD (National Digital Forecast Database) — the NWS's own forecaster-blended gridded
//! forecast, read directly from the public `noaa-ndfd-pds` S3 bucket.
//!
//! Every other gridded model this app reads publishes one file per valid time, with many
//! variables bundled inside and an index sidecar to slice out just the one wanted
//! (`hrrr::fetch_field`'s byte-range fetch). NDFD is the opposite shape: one file per
//! variable, with every forecast hour over the next several days bundled together, and no
//! index at all. So this always fetches the whole file and keeps the one message valid
//! nearest to now — there is no forecast-hour scrub for these grids, the same "always
//! current" shape the HRRR CAPE/SRH environment layers already have.

use crate::mrms::MrmsField;
use chrono::{DateTime, Utc};

const BUCKET: &str = "https://noaa-ndfd-pds.s3.amazonaws.com";

/// NDFD's CONUS grid is ~2.5 km, the same native resolution as the NBM blend it feeds into.
const RES_DEG: f64 = 0.035;

/// One of NDFD's published elements, each its own `ds.<elem>.bin` file under the CONUS
/// short-range (day 1-3) directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NdfdField {
    /// 2 m temperature.
    Temp,
    /// 10 m sustained wind speed.
    WindSpeed,
    /// 10 m wind gust.
    WindGust,
    /// Forecast snowfall accumulation (liquid-equivalent adjusted).
    Snow,
}

impl NdfdField {
    fn file(self) -> &'static str {
        match self {
            NdfdField::Temp => "ds.temp.bin",
            NdfdField::WindSpeed => "ds.wspd.bin",
            NdfdField::WindGust => "ds.wgust.bin",
            NdfdField::Snow => "ds.snow.bin",
        }
    }
}

/// Fetch `field`'s CONUS short-range bundle and decode the message valid nearest to now.
pub async fn fetch(client: &reqwest::Client, field: NdfdField) -> anyhow::Result<MrmsField> {
    let url = format!("{BUCKET}/opnl/AR.conus/VP.001-003/{}", field.file());
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
    decode_nearest(&bytes, Utc::now())
}

/// Pick the GRIB2 message in `bytes` whose forecast valid time is closest to `now`, and
/// regrid it. A short-range NDFD file bundles on the order of a hundred forecast hours; a
/// linear scan through all of them is the only option with no index to seek by.
fn decode_nearest(bytes: &[u8], now: DateTime<Utc>) -> anyhow::Result<MrmsField> {
    use gribberish::message::read_messages;

    let best = read_messages(bytes)
        .filter_map(|m| {
            let t = m.forecast_date().ok()?;
            Some(((t - now).num_seconds().abs(), t, m))
        })
        .min_by_key(|(dt, ..)| *dt);
    let (_, time, msg) = best.ok_or_else(|| anyhow::anyhow!("no usable GRIB2 message in NDFD file"))?;

    use gribberish::data_message::DataMessage;
    let dm = DataMessage::try_from(&msg).map_err(|e| anyhow::anyhow!("ndfd decode: {e:?}"))?;
    let (lats, lons) = dm.metadata.latlng();
    anyhow::ensure!(
        lats.len() == dm.data.len() && lons.len() == dm.data.len(),
        "ndfd latlng/data length mismatch"
    );

    crate::hrrr::regrid(&lats, &lons, &dm.data, time, RES_DEG, f64::NEG_INFINITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real ASNOW (snowfall) message from `ds.snow.bin`, Complex Grid Packing (data
    /// representation template 5.2, no spatial differencing) — the one packing this app reads
    /// that nothing else so far had exercised, and it panicked every single message before two
    /// bugs in `vendor/gribberish`'s template got fixed: a plain `.load()` instead of
    /// `.load_be()` on group references/widths (silently wrong on a little-endian host, since
    /// this is a big-endian-ordered bitstream), and trusting the reference+increment formula for
    /// the *last* group's length instead of reading it explicitly like the spec requires (the
    /// sibling spatial-differencing template already got this second part right). Regression
    /// coverage for both without a network fetch.
    #[test]
    fn a_complex_packed_ndfd_message_decodes_without_panicking() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/regression/ndfd_snow_complex_packing.grib2");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let msg = gribberish::message::read_message(&bytes, 0).expect("a GRIB2 message");
        assert_eq!(msg.data_compression_type().unwrap(), "Complex Grid Packing");
        let dm = gribberish::data_message::DataMessage::try_from(&msg).expect("decode");
        assert_eq!(dm.metadata.units, "m");
        assert!(!dm.data.is_empty());
        let finite: Vec<f64> = dm.data.iter().copied().filter(|v| v.is_finite()).collect();
        assert!(!finite.is_empty(), "every value came back non-finite");
        for &v in &finite {
            assert!((0.0..50.0).contains(&v), "implausible snow depth {v} m");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_and_regrids_live_temp() {
        let client = reqwest::Client::new();
        let field = fetch(&client, NdfdField::Temp).await.expect("fetch");
        eprintln!(
            "NDFD temp: {}x{} lon {:.1}..{:.1} lat {:.1}..{:.1} at {}",
            field.nx, field.ny, field.lon_west, field.lon_east, field.lat_south, field.lat_north,
            field.time
        );
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 4, "too few finite cells: {finite}/{}", field.values.len());
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!((150.0..350.0).contains(&v), "implausible temperature {v} K");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_and_regrids_live_wind_speed() {
        let client = reqwest::Client::new();
        let field = fetch(&client, NdfdField::WindSpeed).await.expect("fetch");
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 4, "too few finite cells: {finite}/{}", field.values.len());
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!((0.0..100.0).contains(&v), "implausible wind speed {v} m/s");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_and_regrids_live_wind_gust() {
        let client = reqwest::Client::new();
        let field = fetch(&client, NdfdField::WindGust).await.expect("fetch");
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 4, "too few finite cells: {finite}/{}", field.values.len());
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!((0.0..120.0).contains(&v), "implausible wind gust {v} m/s");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_and_regrids_live_snow() {
        let client = reqwest::Client::new();
        let field = fetch(&client, NdfdField::Snow).await.expect("fetch");
        let finite = field.values.iter().filter(|v| v.is_finite()).count();
        assert!(finite > field.values.len() / 4, "too few finite cells: {finite}/{}", field.values.len());
        for &v in field.values.iter().filter(|v| v.is_finite()) {
            assert!((0.0..20.0).contains(&v), "implausible snow depth {v} m");
        }
    }
}
