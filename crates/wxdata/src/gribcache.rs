//! Byte-range cache for GRIB2 messages.
//!
//! Every model, RTMA and GEFS fetch is the same shape: read the `.idx`, then range-GET one message
//! out of a multi-hundred-MB file. A given `(url, byte range)` never changes once its run is
//! published, so the message can be kept and handed back without a request. That makes scrubbing
//! back over lead times, flipping between products and re-opening a run free after the first read.
//!
//! Storage is [`crate::objcache`]'s, shared with every other immutable download: a disk store on
//! the desktop and phone, IndexedDB in the browser, none in headless tools and tests. Entries are
//! namespaced by [`Family`] so each source family has its own size quota.
//!
//! Nothing is trusted on the way in or out. A message is only kept when it is the length the range
//! asked for and reads as a complete GRIB2 message (`GRIB` in front, `7777` behind), and a stored
//! entry that no longer checks out is ignored and replaced. A run still being written to the
//! bucket, or a connection cut short, therefore never poisons the cache.

/// Which source a message belongs to; the unit of the cache's namespacing and size quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// HRRR, including the sub-hourly files.
    Hrrr,
    /// GEFS ensemble members and their mean/spread.
    Gefs,
    /// RTMA surface analyses.
    Rtma,
    /// Every other model (GFS, NAM, ECMWF open data, ...).
    Models,
}

impl Family {
    pub const ALL: [Family; 4] = [Family::Hrrr, Family::Gefs, Family::Rtma, Family::Models];

    /// The folder name, stable on disk.
    pub fn slug(self) -> &'static str {
        match self {
            Family::Hrrr => "hrrr",
            Family::Gefs => "gefs",
            Family::Rtma => "rtma",
            Family::Models => "models",
        }
    }

    /// A person-facing name for the Storage tab.
    pub fn label(self) -> &'static str {
        match self {
            Family::Hrrr => "HRRR GRIB",
            Family::Gefs => "GEFS GRIB",
            Family::Rtma => "RTMA GRIB",
            Family::Models => "Other model GRIB",
        }
    }

    /// The cache space this family's messages are kept in.
    pub fn space(self) -> &'static crate::objcache::Space {
        use crate::objcache as oc;
        match self {
            Family::Hrrr => &oc::GRIB_HRRR,
            Family::Gefs => &oc::GRIB_GEFS,
            Family::Rtma => &oc::GRIB_RTMA,
            Family::Models => &oc::GRIB_MODELS,
        }
    }

    /// Which family a GRIB file URL belongs to. Checked most specific first: a GEFS path also
    /// contains `gefs`-only names, and HRRR sub-hourly paths contain `hrrr`.
    pub fn of_url(url: &str) -> Family {
        let u = url.to_ascii_lowercase();
        if u.contains("gefs") {
            Family::Gefs
        } else if u.contains("rtma") {
            Family::Rtma
        } else if u.contains("hrrr") {
            Family::Hrrr
        } else {
            Family::Models
        }
    }
}

/// The cache key for a range of a URL. The whole URL is in it, and therefore the run date and
/// cycle, so a new run never collides with an old one.
pub fn key(url: &str, range: (u64, Option<u64>)) -> String {
    match range.1 {
        Some(e) => format!("{url}#{}-{e}", range.0),
        None => format!("{url}#{}-", range.0),
    }
}

/// The HTTP `Range` header value for a half-open `[start, end)` byte range (`end` `None` = to EOF).
pub fn range_header(range: (u64, Option<u64>)) -> String {
    match range.1 {
        Some(e) => format!("bytes={}-{}", range.0, e.saturating_sub(1)),
        None => format!("bytes={}-", range.0),
    }
}

/// Whether `bytes` is a complete answer for `range`: exactly as long as asked when the range has
/// an end, and a whole GRIB2 message (or run of them) either way.
pub fn is_complete(bytes: &[u8], range: (u64, Option<u64>)) -> bool {
    if let Some(e) = range.1 {
        if bytes.len() as u64 != e.saturating_sub(range.0) {
            return false;
        }
    }
    bytes.len() >= 12 && bytes.starts_with(b"GRIB") && bytes.ends_with(b"7777")
}

/// Range-GET one GRIB2 message from `url`, from the cache when it is there.
///
/// Only a complete answer is cached ([`is_complete`]); an incomplete one is still returned, so the
/// caller's decode sees exactly what it always did.
pub async fn fetch_range(
    http: &reqwest::Client,
    url: &str,
    range: (u64, Option<u64>),
    user_agent: &str,
) -> anyhow::Result<Vec<u8>> {
    let family = Family::of_url(url);
    crate::objcache::cached(
        family.space(),
        &key(url, range),
        |b| is_complete(b, range),
        async {
            Ok(http
                .get(crate::net::fetch_url(url))
                .timeout(crate::net::FEED_TIMEOUT)
                .header("User-Agent", user_agent)
                .header("Range", range_header(range))
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
                .to_vec())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len];
        v[..4].copy_from_slice(b"GRIB");
        v[len - 4..].copy_from_slice(b"7777");
        v
    }

    #[test]
    fn families_are_told_apart_by_url() {
        let of = Family::of_url;
        assert_eq!(
            of("https://noaa-gefs-pds.s3.amazonaws.com/gefs.20260101/00/atmos/pgrb2sp25/geavg.t00z.pgrb2s.0p25.f006"),
            Family::Gefs
        );
        assert_eq!(
            of("https://noaa-rtma-pds.s3.amazonaws.com/rtma2p5.20260101/rtma2p5.t00z.2dvaranl_ndfd.grb2"),
            Family::Rtma
        );
        assert_eq!(
            of("https://noaa-hrrr-bdp-pds.s3.amazonaws.com/hrrr.20260101/conus/hrrr.t00z.wrfsubhf00.grib2"),
            Family::Hrrr
        );
        assert_eq!(
            of("https://noaa-gfs-bdp-pds.s3.amazonaws.com/gfs.20260101/00/atmos/gfs.t00z.pgrb2.0p25.f006"),
            Family::Models
        );
    }

    #[test]
    fn keys_separate_runs_and_ranges() {
        let a = key("https://x/a.f006", (100, Some(200)));
        assert_ne!(a, key("https://x/a.f006", (100, Some(201))));
        assert_ne!(a, key("https://x/a.f006", (101, Some(200))));
        assert_ne!(a, key("https://x/b.f006", (100, Some(200))));
        assert_ne!(
            key("https://x/a", (5, None)),
            key("https://x/a", (5, Some(9)))
        );
        assert_ne!(
            crate::objcache::digest(&a),
            crate::objcache::digest("other")
        );
        assert_eq!(crate::objcache::digest(&a).len(), 32);
    }

    #[test]
    fn each_family_has_its_own_space() {
        let mut slugs: Vec<_> = Family::ALL.iter().map(|f| f.space().slug).collect();
        slugs.dedup();
        assert_eq!(slugs, ["hrrr", "gefs", "rtma", "models"]);
    }

    #[test]
    fn the_range_header_is_inclusive_of_the_last_byte() {
        assert_eq!(range_header((100, Some(200))), "bytes=100-199");
        assert_eq!(range_header((100, None)), "bytes=100-");
    }

    #[test]
    fn only_whole_messages_of_the_asked_length_are_complete() {
        let m = msg(64);
        assert!(is_complete(&m, (1000, Some(1064))));
        assert!(is_complete(&m, (1000, None)));
        // Truncated by a dropped connection: right start, wrong length.
        assert!(!is_complete(&m[..40], (1000, Some(1064))));
        // A run still being written reads as zeros where the message will be.
        assert!(!is_complete(&[0u8; 64], (1000, Some(1064))));
        // An error page in place of a message.
        assert!(!is_complete(
            b"<Error>SlowDown please retry later</Error>",
            (0, None)
        ));
        // No trailer: cut inside an open-ended read.
        assert!(!is_complete(&m[..60], (1000, None)));
        assert!(!is_complete(&[], (0, None)));
    }
}
