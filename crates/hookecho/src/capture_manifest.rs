//! The capture manifest (ROADMAP_PARITY M6.3): what a loop export actually captured, frame by
//! frame, written beside the video as its JSON sidecar.
//!
//! A logical frame is one weather moment; an encoded frame is one picture in the file. The
//! manifest maps one to the other (a fixed-rate MP4 holds a frame for several encoded frames on
//! purpose), records which scan each logical frame asked for and which was on screen when it was
//! captured, how long that took, every field layer's readiness and valid time at that moment, and
//! a SHA-256 of the captured pixels, so a repeat run can be compared and a substituted, duplicated
//! or half-loaded frame is a listed problem rather than a silent one.
use chrono::{DateTime, Utc};
use serde::Serialize;

/// Bumped when a field changes meaning; readers reject a newer one.
pub const MANIFEST_SCHEMA: u32 = 1;

/// One field layer as it stood when a frame was captured.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceStamp {
    /// The layer's stable slug.
    pub layer: String,
    /// Whether the layer held data for the frame's selected time.
    pub ready: bool,
    /// The data's own valid time, when it has one.
    pub valid_time_utc: Option<DateTime<Utc>>,
    /// Seconds from the frame's radar time to the layer's valid time (positive: the layer is
    /// later). `None` without both times.
    pub offset_s: Option<i64>,
}

/// What capturing one logical frame found, before encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameRecord {
    /// The scan the timeline asked for.
    pub asked: Option<String>,
    /// The scan on screen when the picture was taken, and its valid time.
    pub shown: Option<(String, DateTime<Utc>)>,
    /// From the step to the screenshot request, ms.
    pub waited_ms: u64,
    /// The asked scan never appeared within the wait and what was shown was captured.
    pub timed_out: bool,
    pub sources: Vec<SourceStamp>,
    /// SHA-256 of the captured RGBA pixels, hex.
    pub sha256: String,
}

/// One logical frame in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ManifestFrame {
    pub index: usize,
    pub asked: Option<String>,
    pub shown: Option<String>,
    pub valid_time_utc: Option<DateTime<Utc>>,
    /// When it starts and how long it is held in the output, ms.
    pub start_ms: u64,
    pub delay_ms: u32,
    /// Encoded pictures it occupies (always 1 in a GIF; the constant-rate MP4 repeats frames to
    /// fill a hold), and the first one's index.
    pub encoded_frames: u32,
    pub first_encoded: u64,
    pub waited_ms: u64,
    pub timed_out: bool,
    pub sha256: String,
    pub sources: Vec<SourceStamp>,
}

/// SHA-256 of RGBA pixels, lowercase hex.
pub fn checksum(rgba: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(rgba)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The layer readings for a frame: each `(slug, ready, valid time)` against the frame's radar
/// time.
pub fn stamps(
    radar_time: Option<DateTime<Utc>>,
    layers: impl IntoIterator<Item = (String, bool, Option<DateTime<Utc>>)>,
) -> Vec<SourceStamp> {
    layers
        .into_iter()
        .map(|(layer, ready, valid)| SourceStamp {
            layer,
            ready,
            valid_time_utc: valid,
            offset_s: radar_time.zip(valid).map(|(r, v)| (v - r).num_seconds()),
        })
        .collect()
}

/// The logical frames with their holds and encoded-frame mapping: `encoded` is the pictures per
/// frame (`loopexport::cfr_counts` for MP4, ones for a GIF).
pub fn frames(records: &[FrameRecord], delays_ms: &[u32], encoded: &[u32]) -> Vec<ManifestFrame> {
    let (mut start_ms, mut first_encoded) = (0u64, 0u64);
    records
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let delay_ms = delays_ms.get(index).copied().unwrap_or(0);
            let encoded_frames = encoded.get(index).copied().unwrap_or(1);
            let f = ManifestFrame {
                index,
                asked: r.asked.clone(),
                shown: r.shown.as_ref().map(|(name, _)| name.clone()),
                valid_time_utc: r.shown.as_ref().map(|(_, t)| *t),
                start_ms,
                delay_ms,
                encoded_frames,
                first_encoded,
                waited_ms: r.waited_ms,
                timed_out: r.timed_out,
                sha256: r.sha256.clone(),
                sources: r.sources.clone(),
            };
            start_ms += u64::from(delay_ms);
            first_encoded += u64::from(encoded_frames);
            f
        })
        .collect()
}

/// What went wrong, frame by frame, in words: a frame that is not the scan it asked for, the
/// same scan captured for two different logical frames, and layers that were not ready.
pub fn problems(records: &[FrameRecord]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let n = i + 1;
        let shown = r.shown.as_ref().map(|(s, _)| s.as_str());
        match (r.asked.as_deref(), shown) {
            (Some(a), Some(s)) if a != s => out.push(format!(
                "frame {n}: asked for {a}, captured {s}{}",
                if r.timed_out {
                    " after the wait ran out"
                } else {
                    ""
                }
            )),
            (Some(a), None) => out.push(format!("frame {n}: asked for {a}, nothing was shown")),
            _ => {}
        }
        if i > 0 {
            let prev = &records[i - 1];
            let prev_shown = prev.shown.as_ref().map(|(s, _)| s.as_str());
            if shown.is_some() && shown == prev_shown && r.asked != prev.asked {
                out.push(format!(
                    "frame {n}: same scan as frame {i} ({})",
                    shown.unwrap_or_default()
                ));
            }
        }
        let missing: Vec<&str> = r
            .sources
            .iter()
            .filter(|s| !s.ready)
            .map(|s| s.layer.as_str())
            .collect();
        if !missing.is_empty() {
            out.push(format!("frame {n}: not ready: {}", missing.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 20, 20, min, 0).unwrap()
    }

    fn record(asked: &str, shown: &str, min: u32) -> FrameRecord {
        FrameRecord {
            asked: Some(asked.into()),
            shown: Some((shown.into(), t(min))),
            waited_ms: 400,
            timed_out: false,
            sources: stamps(
                Some(t(min)),
                [("mrms-mesh".to_string(), true, Some(t(min)))],
            ),
            sha256: checksum(asked.as_bytes()),
        }
    }

    #[test]
    fn the_checksum_is_sha256_of_the_pixels() {
        assert_eq!(
            checksum(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(checksum(&[0, 0, 0, 255]), checksum(&[0, 0, 1, 255]));
    }

    #[test]
    fn logical_frames_map_onto_encoded_frames_without_drift() {
        let records = vec![
            record("A", "A", 0),
            record("B", "B", 4),
            record("C", "C", 9),
        ];
        let delays = [500u32, 500, 1500];
        let encoded = crate::loopexport::cfr_counts(&delays, crate::loopexport::MP4_FPS);
        let f = frames(&records, &delays, &encoded);
        assert_eq!(
            f.iter()
                .map(|f| (f.start_ms, f.first_encoded, f.encoded_frames))
                .collect::<Vec<_>>(),
            [(0, 0, 15), (500, 15, 15), (1000, 30, 45)]
        );
        // A GIF is one picture per frame.
        let g = frames(&records, &delays, &[1, 1, 1]);
        assert_eq!(g[2].first_encoded, 2);
        // Valid times and offsets come from what was shown.
        assert_eq!(f[1].valid_time_utc, Some(t(4)));
        assert_eq!(f[1].sources[0].offset_s, Some(0));
        assert!(problems(&records).is_empty());
    }

    #[test]
    fn substitutions_duplicates_and_unready_layers_are_listed() {
        let mut late = record("C", "B", 4);
        late.timed_out = true;
        let mut unready = record("D", "D", 14);
        unready.sources = stamps(
            Some(t(14)),
            [
                ("goes-ir".to_string(), false, None),
                ("mrms-mesh".to_string(), true, Some(t(16))),
            ],
        );
        let records = vec![record("A", "A", 0), record("B", "B", 4), late, unready];
        let p = problems(&records);
        assert_eq!(p.len(), 3, "{p:?}");
        assert_eq!(
            p[0],
            "frame 3: asked for C, captured B after the wait ran out"
        );
        assert_eq!(p[1], "frame 3: same scan as frame 2 (B)");
        assert_eq!(p[2], "frame 4: not ready: goes-ir");
        assert_eq!(records[3].sources[1].offset_s, Some(120));
    }
}
