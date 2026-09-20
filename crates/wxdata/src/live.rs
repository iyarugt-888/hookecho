//! Live Level 2 chunk streaming.
//!
//! NEXRAD publishes a volume as a sequence of small "chunks" to an S3 bucket during the scan
//! itself, so a display can update sweep-by-sweep instead of waiting ~5 min for the archived
//! volume. [`stream`] drives `nexrad-data`'s pull-based [`ChunkIterator`], assembles the
//! newest chunk into a [`Scan`] as it arrives, merges it into the running
//! volume, and hands the caller a full updated [`Scan`] via `on_update`.
//!
//! All merged state lives on this task; the UI thread only ever receives a finished `Scan`.
//!
//! [`ScanProgress`] is the lighter-weight sibling: `on_progress` also fires on every chunk before
//! the merged [`Update`], so a UI can show how far into the current tilt the radar has scanned. It carries no
//! scan data and costs nothing to compute — the chunk's own metadata already has the answer —
//! so this does not change how often the expensive reassembly in `emit` runs (Phase B2's
//! "expose current elevation, VCP, sweep number and scan progress").

use crate::level2::{elevation_angles, Scan};
use nexrad_data::aws::realtime::{
    assemble_volume, download_chunk, Chunk, ChunkIdentifier, ChunkIterator, ChunkTimingModel,
    ChunkType,
};
use nexrad_model::data::{Radial, Sweep};
use std::sync::Arc;
use std::time::Duration;

/// How far the live stream has scanned into the volume right now, independent of whether a
/// merged [`Update`] has arrived for it yet — the Start chunk (metadata only, no elevation of
/// its own) never produces one of these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanProgress {
    /// 1-based position of the current sweep within the VCP.
    pub elevation_number: usize,
    /// How many sweeps this VCP has in total.
    pub total_elevations: usize,
    pub elevation_angle_deg: f64,
    /// Antenna rotation rate declared by this VCP cut, in degrees per second.
    pub azimuth_rate_dps: f64,
    /// Clockwise azimuth sector refreshed by this update. `end < start` crosses north.
    pub azimuth_start_deg: f64,
    pub azimuth_end_deg: f64,
    /// 1-based position of the chunk just received within its sweep.
    pub chunk_index: usize,
    /// How many chunks this sweep has in total (3 standard, 6 super-resolution).
    pub chunks_in_sweep: usize,
}

impl ScanProgress {
    pub fn azimuth_span_deg(self) -> f32 {
        let start = self.azimuth_start_deg as f32;
        let end = self.azimuth_end_deg as f32;
        if !start.is_finite() || !end.is_finite() {
            return 0.0;
        }
        let raw = end - start;
        if raw.abs() >= 359.999 {
            360.0
        } else {
            raw.rem_euclid(360.0)
        }
    }

    /// Physics-based duration of this chunk's azimuth sector. Uses the same empirically corrected
    /// VCP timing model that schedules the live downloader; four seconds is its documented
    /// fallback when a provider cannot supply a valid rotation rate.
    pub fn chunk_duration_secs(self) -> f32 {
        let span = self.azimuth_span_deg();
        let nominal_span = if self.chunks_in_sweep > 0 {
            360.0 / self.chunks_in_sweep as f32
        } else {
            0.0
        };
        let nominal =
            ChunkTimingModel::chunk_duration_secs(self.azimuth_rate_dps, self.chunks_in_sweep)
                .unwrap_or(4.0) as f32;
        if span > 0.0 && nominal_span > 0.0 {
            nominal * span / nominal_span
        } else {
            nominal
        }
    }
}

/// A merged live volume ready to display.
pub struct Update {
    /// A synthetic name identifying this update (volume prefix + sequence).
    pub name: String,
    pub time: chrono::DateTime<chrono::Utc>,
    /// Shared with the streaming task's running volume — the app puts this straight into its
    /// volume cache, so a live sweep arrival copies a refcount rather than a whole scan.
    pub scan: Arc<Scan>,
    /// Elevation angles (deg) whose sweeps changed vs. the previous update — the app uses
    /// this to evict only the affected tilts from its binned-sweep cache.
    pub changed: Vec<f32>,
    /// Chunk fetch failures retried (not dropped — a stream that runs out of retries ends
    /// instead, see [`tolerate_failure`]) since this stream connection started. Zero for a
    /// perfectly healthy connection; a rising count on an otherwise-working stream is a sign of a
    /// flaky network, surfaced so that isn't invisible the way it was before this field existed.
    pub retries: u32,
    /// Wall clock spent assembling and merging this update — the local half of live latency, as
    /// opposed to the provider's own ingest lag (how stale the data already was on arrival). See
    /// [`emit`]'s own comment for exactly what this does and doesn't include.
    pub decode_time: std::time::Duration,
}

/// Stream live chunks for `site`, starting from `base` (the last polled volume), calling
/// `on_update` with a full merged [`Scan`] after each chunk.
///
/// `active` is polled before each chunk fetch; returning false ends the stream cleanly, so a
/// backgrounded phone stops pulling chunks over mobile data and stops holding a timer awake (the
/// app passes its foreground gate and restarts the stream on resume), and so a caller with no way
/// to abort a spawned task can still stop one. A closure rather than a `fn` pointer for that
/// second reason: the caller needs to capture the generation counter it cancels with. This crate
/// still knows nothing about windowing or platforms.
///
/// Returns `Ok(())` only if the iterator ends cleanly (it normally runs until aborted);
/// any error returns so the caller can fall back to interval polling.
///
/// Runs on the web too: the waits go through [`crate::task::sleep`] (a `setTimeout` there) and the
/// backfill through `futures_util`, so nothing in here reaches for tokio directly.
pub async fn stream<F, P>(
    site: String,
    base: Arc<Scan>,
    active: impl Fn() -> bool,
    mut on_update: F,
    mut on_progress: P,
) -> anyhow::Result<()>
where
    F: FnMut(Update),
    P: FnMut(ScanProgress),
{
    let init = ChunkIterator::start(&site)
        .await
        .map_err(|e| anyhow::anyhow!("chunk iterator start: {e}"))?;
    let mut it = init.iterator;

    // Assemble the current volume: start chunk + backfilled middle chunks + the joined chunk.
    let mut chunks: Vec<Chunk<'static>> = Vec::new();
    if let Some(sc) = init.start_chunk {
        chunks.push(sc.chunk);
    }
    let joined = &init.latest_chunk.identifier;
    let latest_seq = joined.sequence();
    let volume = *joined.volume();
    let prefix = *joined.date_time_prefix();
    // Backfill the middle chunks so the first frame is a full volume. Up to ~53 of them, and
    // serially that was the whole reason a live site took seconds to show anything; six at a time
    // keeps the radio busy without opening a connection per chunk. Gaps are tolerated (a missing
    // chunk just omits its radials), but order matters, so results are re-sorted by sequence.
    // `join_all` rather than a tokio `JoinSet`: these are pure I/O waits, so one task driving six
    // of them is the same wall clock, and it is the only shape the single-threaded web build has.
    let mut backfill: Vec<(usize, Chunk<'static>)> = Vec::new();
    for window in (2..latest_seq).collect::<Vec<_>>().chunks(6) {
        let gets = window.iter().map(|&seq| {
            let site = site.clone();
            async move {
                let id = ChunkIdentifier::new(
                    site.clone(),
                    volume,
                    prefix,
                    seq,
                    ChunkType::Intermediate,
                    None,
                );
                download_chunk(&site, &id)
                    .await
                    .ok()
                    .map(|(_, ch)| (seq, ch))
            }
        });
        backfill.extend(
            futures_util::future::join_all(gets)
                .await
                .into_iter()
                .flatten(),
        );
    }
    backfill.sort_by_key(|(seq, _)| *seq);
    chunks.extend(backfill.into_iter().map(|(_, ch)| ch));
    chunks.push(init.latest_chunk.chunk);

    let mut merged = base;
    let mut volume = init.latest_chunk.identifier.volume().as_number();
    // First emit assembles the whole backfilled volume; after that only the chunks since the last
    // sweep boundary are re-assembled (plus the start chunk, which carries the VCP and site
    // metadata assembly needs). Re-decoding every accumulated chunk at every boundary was O(n^2)
    // over a volume, and the chunk count grows to ~55.
    let mut total_retries = 0u32;
    emit(&it, &chunks, &mut merged, total_retries, &mut on_update).await;
    let mut window_start = chunks.len();

    let mut fails = 0u32;
    loop {
        let wait = it
            .time_until_next()
            .and_then(|d| d.to_std().ok())
            .unwrap_or(Duration::from_secs(2))
            .clamp(Duration::from_secs(1), Duration::from_secs(15));
        // Sliced, not one long sleep: a cancelled stream that only notices at the end of a
        // fifteen-second wait keeps pulling chunks for a site the user already left.
        if !crate::task::sleep_while(wait, &active).await {
            // Backgrounded: end the stream rather than idle in it. Idling would keep a timer
            // (and this task) alive across the whole background period; the caller restarts the
            // stream when the app comes back, which is what it already does after any other
            // stream end.
            return Ok(());
        }

        match it.try_next().await {
            Ok(Some(dc)) => {
                fails = 0;
                let seq = dc.identifier.sequence();
                let ctype = dc.identifier.chunk_type();
                let vol = dc.identifier.volume().as_number();
                // Start chunk is the normal rollover marker, but a stream that joins mid-volume
                // (or misses the Start) would otherwise keep assembling the previous volume's
                // chunks alongside the new one's.
                if ctype == ChunkType::Start || vol != volume {
                    chunks.clear(); // volume rollover: start a fresh accumulator
                    window_start = 0;
                }
                volume = vol;
                chunks.push(dc.chunk);
                let meta = it.chunk_metadata(seq).copied();
                if let Some(meta) = meta {
                    // The Start chunk has no elevation of its own; nothing to report yet.
                    if let Some(elevation_number) = meta.elevation_number() {
                        on_progress(ScanProgress {
                            elevation_number,
                            total_elevations: it
                                .elevation_mapper()
                                .map_or(elevation_number, |m| m.total_elevations()),
                            elevation_angle_deg: meta.elevation_angle_deg(),
                            azimuth_rate_dps: meta.azimuth_rate_dps(),
                            chunk_index: meta.chunk_index_in_sweep() + 1,
                            chunks_in_sweep: meta.chunks_in_sweep(),
                            azimuth_start_deg: meta.chunk_index_in_sweep() as f64 * 360.0
                                / meta.chunks_in_sweep() as f64,
                            azimuth_end_deg: (meta.chunk_index_in_sweep() + 1) as f64 * 360.0
                                / meta.chunks_in_sweep() as f64,
                        });
                    }
                }
                // Phase B2 / suggestions.md §21: every chunk is a rendering unit, not just the
                // one that finishes a sweep. A chunk is ~120 radials — a 60° wedge of super-res —
                // so waiting for all six of them held the display a whole rotation (15 s in a
                // precipitation VCP, over a minute in clear air) behind data already on this
                // machine. `merge_scan`/`stitch` already merge partial sweeps by azimuth, so the
                // only thing that was stopping this was the emit gate itself.
                //
                // The fetch itself can outlast the cancellation: a sweep assembled for a site the
                // caller has already left is a stale frame handed to a live view.
                if active() {
                    let window: Vec<Chunk<'static>> = chunks
                        .first()
                        .filter(|_| window_start > 0)
                        .into_iter()
                        .chain(chunks[window_start..].iter())
                        .cloned()
                        .collect();
                    emit(&it, &window, &mut merged, total_retries, &mut on_update).await;
                    // Advance every emit, not only at sweep boundaries: each window is then the
                    // start chunk plus the one new chunk, so per-chunk emitting costs about the
                    // same total assembly work as the old per-sweep one rather than re-decoding
                    // the whole accumulating sweep each time.
                    window_start = chunks.len();
                }
            }
            Ok(None) => { /* not available yet; loop and wait again */ }
            Err(e) => {
                fails += 1;
                total_retries += 1;
                if !tolerate_failure(fails) {
                    return Err(anyhow::anyhow!("chunk stream: {e}"));
                }
                // A blip (S3 hiccup, laptop lid, Wi-Fi handover) used to kill the stream for
                // good: the caller fell back to polling and its restart re-downloaded the whole
                // ~53-chunk backfill. Retrying in place keeps the iterator and the accumulator.
                log::debug!("chunk stream error ({fails}), retrying: {e}");
                if !crate::task::sleep_while(Duration::from_secs(2), &active).await {
                    return Ok(());
                }
            }
        }
    }
}

/// Pack a chunk window into one buffer: a `u32` little-endian length in front of each payload.
///
/// The Web Worker bridge moves one `ArrayBuffer` per job, and a chunk window is several buffers.
/// A length-prefixed concatenation is the whole protocol — both sides are the same build of the
/// same module, so there is no version to negotiate, exactly as with the postcard `Scan` coming
/// back.
pub fn frame<'a>(payloads: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    for data in payloads {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// Unpack [`frame_chunks`], assemble, and re-encode the partial [`Scan`] as postcard.
///
/// The worker's half of the live path, mirroring `level2::decode_and_encode`. Exported to JS by
/// `hookecho::assemble_live_chunks`; public and target-independent so the framing has a test that
/// runs in CI. (`postcard` is a wasm-only dependency, so this half is too.)
#[cfg(target_arch = "wasm32")]
pub fn assemble_and_encode(framed: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut chunks = Vec::new();
    for data in split_framed(framed)? {
        chunks.push(Chunk::new(data.to_vec()).map_err(|e| anyhow::anyhow!("chunk: {e}"))?);
    }
    let scan = assemble_volume(chunks).map_err(|e| anyhow::anyhow!("assemble: {e}"))?;
    postcard::to_allocvec(&scan).map_err(|e| anyhow::anyhow!("encode scan: {e}"))
}

/// The inverse of [`frame`]: the chunk payloads, borrowed out of `framed`.
///
/// A truncated buffer is an error rather than a short read — the two sides of this are one
/// `postMessage`, so a length that runs off the end means the framing is wrong, not that more is
/// coming.
pub fn split_framed(framed: &[u8]) -> anyhow::Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < framed.len() {
        let len = framed
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| anyhow::anyhow!("framed chunks: truncated length"))?;
        at += 4;
        let data = framed
            .get(at..at + len)
            .ok_or_else(|| anyhow::anyhow!("framed chunks: truncated chunk"))?;
        at += len;
        out.push(data);
    }
    Ok(out)
}

/// Whether a chunk fetch error at `consecutive` failures in a row should be retried rather than
/// ending the stream. Transient network trouble is the common case; a sustained run of failures
/// means the fallback poller should take over.
fn tolerate_failure(consecutive: u32) -> bool {
    consecutive < 5
}

/// Assemble `chunks`, merge into `merged`, and emit if anything changed. Assembly failure
/// (e.g. a still-incomplete volume) is skipped; the next sweep boundary self-heals.
async fn emit<F: FnMut(Update)>(
    it: &ChunkIterator,
    chunks: &[Chunk<'static>],
    merged: &mut Arc<Scan>,
    retries: u32,
    on_update: &mut F,
) {
    // Wall clock around assembly + merge — on native that's real CPU time (off the async worker,
    // see below); on the web it also includes the postMessage round trip to the decode worker, so
    // either way this is an honest answer to "how long did the app wait for usable data," not a
    // narrower "CPU time spent decoding" that would understate the web's actual latency.
    let started = crate::clock::Instant::now();
    // Re-assembling every accumulated chunk at each sweep boundary is the heaviest CPU on this
    // task; `block_in_place` moves it off the async worker so chunk polling and every other
    // fetch on that thread keep running. (Requires the multi-threaded runtime, which is what the
    // app and the headless harness both build.)
    #[cfg(not(target_arch = "wasm32"))]
    let assembled = crate::task::in_place(|| assemble_volume(chunks.iter().cloned()))
        .map_err(|e| anyhow::anyhow!("{e}"));
    // In the browser there is no other thread to move it to: "off the async worker" would be the
    // thread drawing the map, once per sweep boundary for as long as the tab is open. Send the
    // window to the same Web Worker the archive decode uses, and assemble inline only if there
    // is no worker to send it to.
    #[cfg(target_arch = "wasm32")]
    let assembled =
        match crate::wasm_worker::assemble_chunks(frame(chunks.iter().map(|c| c.data()))).await {
            Ok(wire) => crate::level2::scan_from_wire(&wire),
            Err(crate::wasm_worker::Error::Unavailable) => {
                assemble_volume(chunks.iter().cloned()).map_err(|e| anyhow::anyhow!("{e}"))
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
        };
    let partial = match assembled {
        Ok(s) => s,
        Err(e) => {
            log::debug!("assemble skipped: {e}");
            return;
        }
    };
    let (new_scan, changed) = merge_scan(merged, partial);
    if changed.is_empty() {
        return; // nothing new since the last emit; `merged` already holds this content
    }
    *merged = Arc::new(new_scan);
    let (name, time) = it
        .current()
        .map(|id| {
            (
                id.name().to_string(),
                id.upload_date_time().unwrap_or_else(chrono::Utc::now),
            )
        })
        .unwrap_or_else(|| (String::from("live"), chrono::Utc::now()));
    on_update(Update {
        name,
        time,
        scan: Arc::clone(merged),
        changed,
        retries,
        decode_time: started.elapsed(),
    });
}

/// How far behind the newest radial in a tilt an older one may be and still be kept.
///
/// Keeping the previous pass is the point (see [`stitch`]): a half-finished rotation should show
/// the other half of the storm rather than a blank wedge. But a sector the radar has genuinely
/// stopped scanning — a failed volume, a VCP that skips a tilt, a stream that reconnected onto a
/// different elevation set — must not stand there forever pretending to be weather. Fifteen
/// minutes clears the slowest clear-air volume (~10 min) with headroom and bounds the worst case.
/// Anything older than one pass is drawn dimmed and reads its own age in the gate inspector, so
/// this is a backstop, not the thing that keeps stale data honest.
const RETAIN_MS: i64 = 900_000;

/// Stitch a partial sweep onto the base sweep of the same tilt, newest radial wins per azimuth.
///
/// This is both the seam fix and, since partial sweeps became a rendering unit of their own
/// (suggestions.md §21), what makes a half-scanned tilt legible. A chunk can straddle a sweep
/// boundary, so the chunks assembled for one sweep can also carry the first radials of the next
/// one; and with per-chunk emitting, *every* sweep is partial for most of its life. Merging by
/// azimuth covers both: each new radial replaces the one that was at its azimuth, and azimuths
/// the new pass has not reached yet keep showing the previous pass's radials until it does.
///
/// This deliberately no longer drops the previous pass wholesale when a new one starts. That was
/// protecting against last volume's echo standing where the new pass is thin — real, but the fix
/// for it is to *mark* retained data, not to blank the display for most of every rotation. A
/// radial's own `collection_timestamp` is what marks it: [`crate::level2::BinnedSweep`] turns the
/// per-azimuth times into a stale arc the renderer dims and the gate inspector reads.
fn stitch(base: &Sweep, partial: &Sweep) -> Sweep {
    // ponytail: BTreeMap because it dedupes and sorts by azimuth in one pass, and a sweep is
    // ~720 radials. A merge of two already-sorted slices would allocate less, if it ever shows up
    // in a profile.
    let mut by_az: std::collections::BTreeMap<u16, Radial> = base
        .radials()
        .iter()
        .map(|r| (r.azimuth_number(), r.clone()))
        .collect();
    by_az.extend(
        partial
            .radials()
            .iter()
            .map(|r| (r.azimuth_number(), r.clone())),
    );
    let newest = by_az
        .values()
        .map(|r| r.collection_timestamp())
        .max()
        .unwrap_or(0);
    by_az.retain(|_, r| newest - r.collection_timestamp() <= RETAIN_MS);
    Sweep::new(base.elevation_number(), by_az.into_values().collect())
}

/// Merge `partial` into `base`, newest-wins by elevation number.
///
/// A VCP change replaces the volume wholesale (tilt set changed). Otherwise each partial
/// sweep replaces the base sweep with the same elevation number only when it actually differs
/// (`Sweep: PartialEq`), keeping split cuts and the tilt list stable mid-stream. Returns the
/// merged scan and the angles of the sweeps that changed.
/// ponytail: `base`'s sweeps are cloned into the merged scan because `nexrad_model::Scan` has no
/// `into_sweeps` to move them out of. Vendoring that crate for one accessor isn't worth it while
/// this runs off the UI thread.
pub fn merge_scan(base: &Scan, partial: Scan) -> (Scan, Vec<f32>) {
    if base.coverage_pattern_number() != partial.coverage_pattern_number() {
        let changed = elevation_angles(&partial);
        return (partial, changed);
    }

    let mut sweeps: Vec<Sweep> = base.sweeps().to_vec();
    let mut changed_nums: Vec<u8> = Vec::new();
    for ps in partial.sweeps() {
        let en = ps.elevation_number();
        match sweeps.iter().position(|s| s.elevation_number() == en) {
            Some(i) => {
                if &sweeps[i] != ps {
                    sweeps[i] = stitch(&sweeps[i], ps);
                    changed_nums.push(en);
                }
            }
            None => {
                sweeps.push(ps.clone());
                changed_nums.push(en);
            }
        }
    }

    let vcp = base.coverage_pattern().clone();
    let scan = match base.site() {
        Some(s) => Scan::with_site(s.clone(), vcp, sweeps),
        None => Scan::new(vcp, sweeps),
    };
    let changed = changed_nums
        .iter()
        .filter_map(|en| {
            scan.sweeps()
                .iter()
                .find(|s| s.elevation_number() == *en)
                .and_then(|s| s.elevation_angle_degrees())
        })
        .collect();
    (scan, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexrad_model::data::{MomentData, PulseWidth, Radial, RadialStatus, VolumeCoveragePattern};

    fn vcp(n: u16) -> VolumeCoveragePattern {
        VolumeCoveragePattern::new(
            n,
            1,
            0.5,
            PulseWidth::Short,
            false,
            0,
            false,
            0,
            false,
            false,
            0,
            false,
            false,
            Vec::new(),
        )
    }

    #[test]
    fn scan_progress_uses_the_vcp_timing_model_with_documented_fallback() {
        let progress = ScanProgress {
            elevation_number: 1,
            total_elevations: 14,
            elevation_angle_deg: 0.5,
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 60.0,
            chunk_index: 1,
            chunks_in_sweep: 6,
        };
        let expected = ((360.0 / 90.0) - 0.67) / 6.0;
        assert!((progress.chunk_duration_secs() - expected).abs() < 1e-5);
        assert_eq!(
            ScanProgress {
                azimuth_rate_dps: 0.0,
                ..progress
            }
            .chunk_duration_secs(),
            4.0
        );
    }

    // A sweep covering `azimuths` (as azimuth numbers), collected at `t_ms`.
    fn wedge(elevation_number: u8, azimuths: std::ops::Range<u16>, t_ms: i64) -> Sweep {
        let radials = azimuths
            .map(|az| {
                let data = MomentData::from_fixed_point(1, 2125, 250, 8, 2.0, 66.0, vec![100]);
                Radial::new(
                    t_ms,
                    az,
                    az as f32 * 0.5,
                    0.5,
                    RadialStatus::ScanStart,
                    elevation_number,
                    0.5,
                    Some(data),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .collect();
        Sweep::new(elevation_number, radials)
    }

    // A sweep at the given elevation number/angle carrying a single reflectivity value.
    fn sweep(elevation_number: u8, angle: f32, refl_raw: u8) -> Sweep {
        let data = MomentData::from_fixed_point(1, 2125, 250, 8, 2.0, 66.0, vec![refl_raw]);
        let radial = Radial::new(
            0,
            90,
            90.0,
            0.5,
            RadialStatus::ScanStart,
            elevation_number,
            angle,
            Some(data),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        Sweep::new(elevation_number, vec![radial])
    }

    #[test]
    fn merge_replaces_changed_sweep_keeps_others() {
        let base = Scan::new(vcp(212), vec![sweep(1, 0.5, 100), sweep(2, 1.5, 100)]);
        // Partial re-sends tilt 1 unchanged and tilt 2 with new data.
        let partial = Scan::new(vcp(212), vec![sweep(1, 0.5, 100), sweep(2, 1.5, 150)]);
        let (merged, changed) = merge_scan(&base, partial);
        assert_eq!(merged.sweeps().len(), 2);
        // Only tilt 2 (~1.5deg) changed.
        assert_eq!(changed.len(), 1);
        assert!(
            (changed[0] - 1.5).abs() < 0.01,
            "changed angle {:?}",
            changed
        );
    }

    #[test]
    fn merge_appends_new_tilt() {
        let base = Scan::new(vcp(212), vec![sweep(1, 0.5, 100)]);
        let partial = Scan::new(vcp(212), vec![sweep(3, 2.4, 120)]);
        let (merged, changed) = merge_scan(&base, partial);
        assert_eq!(merged.sweeps().len(), 2, "new tilt appended");
        assert_eq!(changed.len(), 1);
    }

    #[test]
    fn vcp_change_replaces_wholesale() {
        let base = Scan::new(vcp(212), vec![sweep(1, 0.5, 100), sweep(2, 1.5, 100)]);
        let partial = Scan::new(vcp(35), vec![sweep(1, 0.5, 100)]);
        let (merged, _) = merge_scan(&base, partial);
        assert_eq!(merged.coverage_pattern_number(), vcp(35).pattern_number());
        assert_eq!(merged.sweeps().len(), 1, "wholesale replace");
    }

    #[test]
    fn a_partial_sweep_does_not_punch_a_hole_in_the_one_already_merged() {
        // The seam: a chunk straddled the boundary, so the first 120 azimuths of tilt 1 were
        // already merged, and the window for this boundary only assembles the rest.
        let base = Scan::new(vcp(212), vec![wedge(1, 0..120, 1_000)]);
        let partial = Scan::new(vcp(212), vec![wedge(1, 120..720, 12_000)]);
        let (merged, changed) = merge_scan(&base, partial);
        assert_eq!(changed.len(), 1);
        let r = merged.sweeps()[0].radials();
        assert_eq!(r.len(), 720, "sweep lost radials — this is the seam");
        assert!(
            r.windows(2)
                .all(|w| w[1].azimuth_number() == w[0].azimuth_number() + 1),
            "radials must stay sorted and gapless"
        );
    }

    /// A new pass over the same tilt keeps the previous one underneath until it sweeps past.
    /// Before partial sweeps were a rendering unit this dropped the old radials wholesale, which
    /// was fine when a tilt only ever reached the display complete — now it would blank five
    /// sixths of the tilt for most of every rotation. What keeps the retained half honest is that
    /// it stays a whole pass behind in time, which the binning turns into a dimmed arc.
    #[test]
    fn a_new_pass_keeps_the_previous_one_in_azimuths_it_has_not_reached() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..720, 1_000)]);
        let partial = Scan::new(vcp(212), vec![wedge(1, 0..120, 301_000)]);
        let (merged, _) = merge_scan(&base, partial);
        let r = merged.sweeps()[0].radials();
        assert_eq!(r.len(), 720, "the whole tilt still has coverage");
        // The 120 azimuths the new pass reached carry its time; the rest still carry the old pass's.
        assert_eq!(r[0].collection_timestamp(), 301_000);
        assert_eq!(r[119].collection_timestamp(), 301_000);
        assert_eq!(r[120].collection_timestamp(), 1_000);
        assert_eq!(r[719].collection_timestamp(), 1_000);
    }

    /// The backstop on the above: a sector the radar stopped scanning altogether does not stand
    /// there indefinitely once it is a quarter-hour behind everything else.
    #[test]
    fn radials_far_older_than_the_newest_are_dropped_rather_than_kept_forever() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..720, 1_000)]);
        let partial = Scan::new(
            vcp(212),
            vec![wedge(1, 0..120, 1_000 + super::RETAIN_MS + 1)],
        );
        let (merged, _) = merge_scan(&base, partial);
        assert_eq!(
            merged.sweeps()[0].radials().len(),
            120,
            "only the new pass survives once the old one is past the retention window"
        );
    }

    #[test]
    fn stitching_prefers_the_newer_radial_for_an_azimuth_it_already_has() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..120, 1_000)]);
        let partial = Scan::new(vcp(212), vec![wedge(1, 60..180, 9_000)]);
        let (merged, _) = merge_scan(&base, partial);
        let r = merged.sweeps()[0].radials();
        assert_eq!(r.len(), 180, "overlap must dedupe, not duplicate");
        assert_eq!(r[60].collection_timestamp(), 9_000);
    }
}

#[cfg(test)]
mod retry_tests {
    use super::{frame, split_framed, tolerate_failure};

    #[test]
    fn transient_failures_retry_then_give_up() {
        assert!(tolerate_failure(1));
        assert!(tolerate_failure(4));
        assert!(!tolerate_failure(5));
        assert!(!tolerate_failure(9));
    }

    /// The framing carries a chunk window across `postMessage` and back. Chunk *contents* are
    /// opaque to it — what has to hold is that n buffers in are the same n buffers out, including
    /// an empty one, which a naive "read until the end" split gets wrong.
    #[test]
    fn framing_round_trips_a_chunk_window() {
        let payloads: Vec<Vec<u8>> = vec![b"AR2V0006.001".to_vec(), vec![], vec![0xff; 300]];
        let framed = frame(payloads.iter().map(|p| p.as_slice()));
        let out = split_framed(&framed).expect("well-formed framing splits");
        assert_eq!(out.len(), payloads.len());
        for (got, want) in out.iter().zip(&payloads) {
            assert_eq!(*got, want.as_slice());
        }
        assert!(split_framed(&framed[..framed.len() - 1]).is_err());
        assert!(split_framed(&framed[..2]).is_err());
        assert!(split_framed(&[])
            .expect("empty framing is an empty window")
            .is_empty());
    }
}
