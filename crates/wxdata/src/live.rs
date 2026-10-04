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
    /// Start of this source volume, independent of the latest radial's time.
    pub volume_start_ms: Option<i64>,
    pub vcp_number: Option<u16>,
    pub cut_kind: CutKind,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CutKind {
    #[default]
    Standard,
    Sails,
    Mrle,
    Mpda,
}

impl CutKind {
    /// A VCP cut's kind from its supplemental flags. Both Level II providers (the decoder's
    /// elevation blocks and the relay's model cuts) expose the same three flags, and both used to
    /// spell this precedence out separately. A cut flagged as more than one reads as the first
    /// of SAILS, MRLE, MPDA; the VCP never sets two.
    pub fn from_flags(sails: bool, mrle: bool, mpda: bool) -> Self {
        if sails {
            Self::Sails
        } else if mrle {
            Self::Mrle
        } else if mpda {
            Self::Mpda
        } else {
            Self::Standard
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Sails => "SAILS",
            Self::Mrle => "MRLE",
            Self::Mpda => "MPDA",
        }
    }
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

/// Radials decoded from the arriving progressive update, before older-pass sweep stitching.
#[derive(Debug, Clone)]
pub struct RadialCoverage {
    pub progress: ScanProgress,
    /// (one-based azimuth number, source acquisition timestamp in milliseconds).
    pub radials: Vec<(u16, i64)>,
    /// Source boundary evidence from raw input, before older-pass stitching. None for callers
    /// that do not expose native boundary markers; it never implies an empty pass history.
    pub source_passes: Option<crate::live_pass::PassInventory>,
    /// Byte-transport receipts, independent of radial positions and scientific pass identity.
    /// Indirection keeps queued progressive channel messages small as receipt evidence grows.
    pub source_sequences: Option<Box<crate::live_sequence::SequenceInventory>>,
}

/// The oldest a joined live volume may be and still be the one being scanned: a volume lasts four
/// to ten minutes, so twenty allows a slow upload and a clear-air VCP.
const MAX_LIVE_VOLUME_AGE_MIN: i64 = 20;

/// `Some(age)` when a volume that started at `started` is too old to be the live one.
pub fn stale_feed(
    started: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::Duration> {
    let age = now - started;
    (age > chrono::Duration::minutes(MAX_LIVE_VOLUME_AGE_MIN)).then_some(age)
}

/// A merged live volume ready to display.
pub struct Update {
    /// A synthetic name identifying this update (volume prefix + sequence).
    pub name: String,
    pub time: chrono::DateTime<chrono::Utc>,
    /// Client transport receipt before assembly. Completed-volume paths may not expose this.
    pub received_at: Option<crate::clock::Instant>,
    /// Current update's raw radial positions; completed-volume sources cannot provide this.
    pub radial_coverage: Option<RadialCoverage>,
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
    // A chunk feed that has stopped publishing still answers with its last volume, which can be
    // days old; streaming that would label yesterday's scan "live". Refuse it, so the caller falls
    // back to polling the archive, which has the current volumes.
    let started = init.latest_chunk.identifier.date_time_prefix().and_utc();
    if let Some(age) = stale_feed(started, chrono::Utc::now()) {
        anyhow::bail!(
            "{site} chunk feed is stale: its newest volume started {} min ago",
            age.num_minutes()
        );
    }

    // Assemble the current volume: start chunk + backfilled middle chunks + the joined chunk.
    let mut chunks: Vec<Chunk<'static>> = Vec::new();
    let start_sequence = init
        .start_chunk
        .as_ref()
        .map(|chunk| chunk.identifier.sequence());
    if let Some(sc) = init.start_chunk {
        chunks.push(sc.chunk);
    }
    let joined = &init.latest_chunk.identifier;
    let latest_seq = joined.sequence();
    let mut sequences = crate::live_sequence::SequenceLedger::new(
        crate::live_sequence::SequenceOrigin::UnidataChunks,
    );
    if let Some(sequence) = start_sequence {
        sequences.observe(sequence as u64);
    } else if joined.chunk_type() != ChunkType::Start {
        // Initialization actually attempted Start 1. Its absence is a failed request receipt,
        // unlike an unrequested prefix on a mid-volume relay join.
        sequences.download_failed(1);
    }
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
                (seq, download_chunk(&site, &id).await)
            }
        });
        // join_all retains request order: concurrent completion order is not source reordering.
        for (seq, result) in futures_util::future::join_all(gets).await {
            match result {
                Ok((_, chunk)) => {
                    sequences.observe(seq as u64);
                    backfill.push((seq, chunk));
                }
                Err(_) => sequences.download_failed(seq as u64),
            }
        }
    }
    backfill.sort_by_key(|(seq, _)| *seq);
    let input_complete = initial_chunks_contiguous(
        start_sequence,
        latest_seq,
        backfill.iter().map(|(seq, _)| *seq),
    );
    let mut prefix_sequence = start_sequence
        .or_else(|| backfill.first().map(|(seq, _)| *seq))
        .or(Some(latest_seq));
    chunks.extend(backfill.into_iter().map(|(_, ch)| ch));
    chunks.push(init.latest_chunk.chunk);
    sequences.observe(latest_seq as u64);

    let mut merged = base;
    let mut volume = init.latest_chunk.identifier.volume().as_number();
    // First emit assembles the whole backfilled volume; after that only the chunks since the last
    // sweep boundary are re-assembled (plus the start chunk, which carries the VCP and site
    // metadata assembly needs). Re-decoding every accumulated chunk at every boundary was O(n^2)
    // over a volume, and the chunk count grows to ~55.
    let mut total_retries = 0u32;
    let mut passes = crate::live_pass::PassTracker::default();
    let first_decoded = emit(
        &it,
        &chunks,
        &mut merged,
        total_retries,
        crate::clock::Instant::now(),
        EmissionProgress {
            cut: None,
            passes: &mut passes,
            sequences: &mut sequences,
            continuous: false,
            input_complete,
        },
        &mut on_update,
    )
    .await;
    let mut last_decoded_sequence = first_decoded
        .then(|| it.current().map(|id| id.sequence()))
        .flatten();
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
                let received_at = crate::clock::Instant::now();
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
                    passes = crate::live_pass::PassTracker::default();
                    sequences = crate::live_sequence::SequenceLedger::new(
                        crate::live_sequence::SequenceOrigin::UnidataChunks,
                    );
                    last_decoded_sequence = None;
                    prefix_sequence = Some(seq);
                }
                volume = vol;
                sequences.observe(seq as u64);
                chunks.push(dc.chunk);
                let meta = it.chunk_metadata(seq).copied();
                let mut current_progress = None;
                if let Some(meta) = meta {
                    // The Start chunk has no elevation of its own; nothing to report yet.
                    if let Some(elevation_number) = meta.elevation_number() {
                        let progress = ScanProgress {
                            volume_start_ms: Some(
                                dc.identifier
                                    .date_time_prefix()
                                    .and_utc()
                                    .timestamp_millis(),
                            ),
                            vcp_number: it.vcp().map(|vcp| vcp.header().pattern_number()),
                            cut_kind: it
                                .vcp()
                                .and_then(|vcp| {
                                    vcp.elevations().get(elevation_number.saturating_sub(1))
                                })
                                .map_or(CutKind::Standard, |cut| {
                                    CutKind::from_flags(
                                        cut.is_sails_cut(),
                                        cut.is_mrle_cut(),
                                        cut.is_mpda_cut(),
                                    )
                                }),
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
                        };
                        on_progress(progress);
                        current_progress = Some(progress);
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
                    let decoded = emit(
                        &it,
                        &window,
                        &mut merged,
                        total_retries,
                        received_at,
                        EmissionProgress {
                            cut: current_progress,
                            passes: &mut passes,
                            sequences: &mut sequences,
                            continuous: last_decoded_sequence.and_then(|old| old.checked_add(1))
                                == Some(seq),
                            input_complete: incremental_input_contiguous(
                                prefix_sequence,
                                seq,
                                last_decoded_sequence.is_some(),
                                window.len() == 1,
                            ),
                        },
                        &mut on_update,
                    )
                    .await;
                    last_decoded_sequence = decoded.then_some(seq);
                    // Advance every emit, not only at sweep boundaries: each window is then the
                    // start chunk plus the one new chunk, so per-chunk emitting costs about the
                    // same total assembly work as the old per-sweep one rather than re-decoding
                    // the whole accumulating sweep each time.
                    window_start = chunks.len();
                }
            }
            Ok(None) => { /* not available yet; loop and wait again */ }
            Err(e) => {
                // try_next does not expose the failed request's object ID; do not predict it.
                sequences.transport_error_without_position();
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
struct EmissionProgress<'a> {
    cut: Option<ScanProgress>,
    passes: &'a mut crate::live_pass::PassTracker,
    sequences: &'a mut crate::live_sequence::SequenceLedger,
    continuous: bool,
    input_complete: bool,
}

/// Check downloaded positions before assembly erases chunk boundaries. Failed middle downloads
/// or an absent Start cannot become an apparently uninterrupted native pass.
fn initial_chunks_contiguous(
    start_sequence: Option<usize>,
    joined_sequence: usize,
    middle: impl Iterator<Item = usize>,
) -> bool {
    joined_sequence >= 1
        && (joined_sequence == 1 || start_sequence == Some(1))
        && middle.eq(2..joined_sequence)
}

fn incremental_input_contiguous(
    prefix_sequence: Option<usize>,
    current_sequence: usize,
    prefix_decoded: bool,
    single_chunk: bool,
) -> bool {
    // A failed metadata-only Start followed by its actual adjacent chunk is still contiguous.
    // Otherwise an old, not-yet-decoded prefix cannot lend a boundary over omitted input.
    single_chunk
        || prefix_decoded
        || prefix_sequence.and_then(|sequence| sequence.checked_add(1)) == Some(current_sequence)
}

/// Backfilled first inputs have no chunk-mapper event. Use the decoded last cut's native
/// geometry for inspection, rather than discarding its raw boundary evidence.
fn raw_progress(scan: &Scan, volume_start_ms: Option<i64>) -> Option<ScanProgress> {
    let sweep = scan
        .sweeps()
        .iter()
        .rev()
        .find(|sweep| !sweep.radials().is_empty())?;
    let elevation_number = usize::from(sweep.elevation_number());
    let cuts = scan.coverage_pattern().elevation_cuts();
    let cut = cuts.get(elevation_number.checked_sub(1)?)?;
    let chunks_in_sweep = if cut.super_resolution_half_degree_azimuth() {
        6
    } else {
        3
    };
    let bins = chunks_in_sweep * 120;
    let first = sweep.radials().first()?.azimuth_number() as usize;
    let last = sweep.radials().last()?.azimuth_number() as usize;
    if !(1..=bins).contains(&first) || !(1..=bins).contains(&last) {
        return None;
    }
    Some(ScanProgress {
        volume_start_ms,
        vcp_number: Some(scan.coverage_pattern_number().number()),
        cut_kind: CutKind::from_flags(cut.is_sails_cut(), cut.is_mrle_cut(), cut.is_mpda_cut()),
        elevation_number,
        total_elevations: cuts.len(),
        elevation_angle_deg: f64::from(sweep.elevation_angle_degrees()?),
        azimuth_rate_dps: cut.azimuth_rate_degrees_per_second(),
        azimuth_start_deg: (first - 1) as f64 * 360.0 / bins as f64,
        azimuth_end_deg: last as f64 * 360.0 / bins as f64,
        chunk_index: (last - 1) / 120 + 1,
        chunks_in_sweep,
    })
}

async fn emit<F: FnMut(Update)>(
    it: &ChunkIterator,
    chunks: &[Chunk<'static>],
    merged: &mut Arc<Scan>,
    retries: u32,
    received_at: crate::clock::Instant,
    progress: EmissionProgress<'_>,
    on_update: &mut F,
) -> bool {
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
            progress.sequences.decode_failed();
            log::debug!("assemble skipped: {e}");
            return false;
        }
    };
    if progress.input_complete {
        progress.passes.observe(&partial, progress.continuous);
    } else {
        progress.passes.observe_discontinuous_assembly(&partial);
    }
    let cut = progress.cut.or_else(|| {
        raw_progress(
            &partial,
            it.current()
                .map(|id| id.date_time_prefix().and_utc().timestamp_millis()),
        )
    });
    let radial_coverage = cut.map(|cut| RadialCoverage {
        progress: cut,
        radials: partial
            .sweeps()
            .iter()
            .filter(|sweep| sweep.elevation_number() as usize == cut.elevation_number)
            .flat_map(|sweep| {
                sweep
                    .radials()
                    .iter()
                    .map(|radial| (radial.azimuth_number(), radial.collection_timestamp()))
            })
            .collect(),
        source_passes: Some(progress.passes.inventory()),
        source_sequences: Some(Box::new(progress.sequences.inventory())),
    });
    let (new_scan, changed) = merge_scan(merged, partial);
    if changed.is_empty() {
        return true; // decoding succeeded; metadata/duplicates do not interrupt source continuity
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
        received_at: Some(received_at),
        radial_coverage,
        scan: Arc::clone(merged),
        changed,
        retries,
        decode_time: started.elapsed(),
    });
    true
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
    for radial in partial.radials() {
        // Chunk order is transport order, not radar time order. A delayed chunk from an
        // earlier pass must not replace a radial we already displayed from a newer pass.
        // On equal timestamps the arriving radial wins, preserving the existing correction
        // behavior for a provider that republishes a gate in the same acquisition instant.
        let entry = by_az.entry(radial.azimuth_number());
        match entry {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(radial.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                if radial.collection_timestamp() >= o.get().collection_timestamp() {
                    o.insert(radial.clone());
                }
            }
        }
    }
    let newest = by_az
        .values()
        .map(|r| r.collection_timestamp())
        .max()
        .unwrap_or(0);
    by_az.retain(|_, r| newest - r.collection_timestamp() <= RETAIN_MS);
    Sweep::new(base.elevation_number(), by_az.into_values().collect())
}

/// Merge `partial` into `base` by elevation number, newest radial wins at each azimuth.
///
/// A VCP change replaces the volume wholesale (tilt set changed). Otherwise each partial
/// sweep is stitched into the base sweep with the same elevation number. Only a real change to
/// the displayed radials is reported, keeping split cuts and the tilt list stable mid-stream.
/// Returns the merged scan and the angles of the sweeps that changed.
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
                    let stitched = stitch(&sweeps[i], ps);
                    if stitched != sweeps[i] {
                        sweeps[i] = stitched;
                        changed_nums.push(en);
                    }
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

    #[test]
    fn initial_chunk_continuity_requires_every_downloaded_position_without_inventing_a_prefix() {
        assert!(initial_chunks_contiguous(None, 1, [].into_iter()));
        assert!(initial_chunks_contiguous(Some(1), 2, [].into_iter()));
        assert!(initial_chunks_contiguous(Some(1), 5, [2, 3, 4].into_iter()));
        assert!(!initial_chunks_contiguous(None, 5, [2, 3, 4].into_iter()));
        assert!(!initial_chunks_contiguous(Some(1), 5, [2, 4].into_iter()));
        assert!(!initial_chunks_contiguous(
            Some(1),
            5,
            [2, 3, 3, 4].into_iter()
        ));
        assert!(!initial_chunks_contiguous(
            Some(1),
            5,
            [4, 3, 2].into_iter()
        ));
        assert!(!initial_chunks_contiguous(Some(1), 0, [].into_iter()));
    }

    #[test]
    fn incremental_prefix_continuity_uses_actual_adjacency_even_after_metadata_only_decode_failure()
    {
        assert!(incremental_input_contiguous(Some(1), 2, false, false));
        assert!(incremental_input_contiguous(Some(500), 501, false, false));
        assert!(!incremental_input_contiguous(Some(1), 5, false, false));
        assert!(!incremental_input_contiguous(None, 5, false, false));
        assert!(!incremental_input_contiguous(
            Some(usize::MAX),
            0,
            false,
            false
        ));
        assert!(incremental_input_contiguous(Some(1), 5, true, false));
        assert!(incremental_input_contiguous(None, 500, false, true));
    }

    #[test]
    fn decoded_backfill_progress_preserves_native_boundary_evidence_without_a_mapper_event() {
        let scan = crate::level2::decode_volume(
            include_bytes!("../tests/data/corpus/mayfield-2021-first-records.ar2").to_vec(),
        )
        .unwrap();
        let p = raw_progress(&scan, Some(1_639_193_029_000)).expect("decoded native cut metadata");
        let sweep = scan.sweeps().last().unwrap();
        assert_eq!(p.elevation_number, sweep.elevation_number() as usize);
        assert_eq!(p.volume_start_ms, Some(1_639_193_029_000));
        assert_eq!(p.vcp_number, Some(scan.coverage_pattern_number().number()));
        assert!((1..=p.chunks_in_sweep).contains(&p.chunk_index));
        assert!(p.azimuth_span_deg() > 0.0);
        assert_eq!(
            p.elevation_angle_deg,
            f64::from(sweep.elevation_angle_degrees().unwrap())
        );
    }

    #[test]
    fn cut_kind_reads_the_supplemental_flags() {
        assert_eq!(CutKind::from_flags(false, false, false), CutKind::Standard);
        assert_eq!(CutKind::from_flags(true, false, false), CutKind::Sails);
        assert_eq!(CutKind::from_flags(false, true, false), CutKind::Mrle);
        assert_eq!(CutKind::from_flags(false, false, true), CutKind::Mpda);
        assert_eq!(CutKind::from_flags(true, true, false), CutKind::Sails);
    }
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
            volume_start_ms: None,
            vcp_number: None,
            cut_kind: CutKind::Standard,
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

    #[test]
    fn reordered_chunk_fills_a_gap_without_replacing_newer_radials() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..120, 9_000)]);
        let late = Scan::new(vcp(212), vec![wedge(1, 60..180, 1_000)]);
        let (merged, changed) = merge_scan(&base, late);
        assert_eq!(changed.len(), 1);
        let radials = merged.sweeps()[0].radials();
        assert_eq!(radials.len(), 180);
        assert_eq!(radials[60].collection_timestamp(), 9_000);
        assert_eq!(radials[119].collection_timestamp(), 9_000);
        assert_eq!(radials[120].collection_timestamp(), 1_000);
    }

    #[test]
    fn repeated_or_older_chunk_does_not_emit_a_false_update() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..120, 9_000)]);
        for time in [9_000, 1_000] {
            let partial = Scan::new(vcp(212), vec![wedge(1, 0..120, time)]);
            let (merged, changed) = merge_scan(&base, partial);
            assert!(changed.is_empty(), "time {time} produced a false update");
            assert_eq!(merged.sweeps()[0].radials(), base.sweeps()[0].radials());
        }
    }

    #[test]
    fn merged_partial_volume_marks_the_previous_pass_for_rendering() {
        let base = Scan::new(vcp(212), vec![wedge(1, 0..720, 1_000_000)]);
        let partial = Scan::new(vcp(212), vec![wedge(1, 0..120, 1_300_000)]);
        let (merged, changed) = merge_scan(&base, partial);
        assert_eq!(changed.len(), 1);
        let binned = crate::level2::bin_sweep_opts(
            &merged.sweeps()[0],
            crate::level2::Moment::Reflectivity,
            35.0,
            -97.0,
            false,
        )
        .expect("synthetic reflectivity sweep should bin");
        assert_eq!(binned.stale_arc_deg, Some((60.0, 0.0)));
    }
}

#[cfg(test)]
mod retry_tests {
    use super::{frame, split_framed, tolerate_failure};

    #[test]
    fn a_volume_older_than_twenty_minutes_is_not_live() {
        let now = chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        assert!(super::stale_feed(now - chrono::Duration::minutes(6), now).is_none());
        assert!(super::stale_feed(now - chrono::Duration::hours(24), now).is_some());
    }

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

#[cfg(test)]
mod live_contract {
    /// The live stream must join the volume being scanned now, not an older pass through the
    /// same reused directory (see the vendored `list_chunks_in_volume`). KVNX's directories held
    /// two passes days apart when the stream was found joining one from the previous day.
    #[tokio::test]
    #[ignore = "network"]
    async fn the_live_stream_joins_the_current_volume() {
        use nexrad_data::aws::realtime::ChunkIterator;
        // Every site either joins a current volume or is recognised as a stopped feed — never a
        // stale volume passed off as live.
        for site in ["KVNX", "KTLX", "KFWS", "KDDC"] {
            let init = ChunkIterator::start(site).await.unwrap();
            let id = &init.latest_chunk.identifier;
            let started = id.date_time_prefix().and_utc();
            let stale = super::stale_feed(started, chrono::Utc::now());
            println!(
                "{site}: volume {} started {started}, stale {stale:?}",
                id.volume().as_number()
            );
            if stale.is_some() {
                // The feed really has stopped: no directory holds anything newer.
                for probe in 1..=999 {
                    let v = nexrad_data::aws::realtime::VolumeIndex::new(probe);
                    if probe % 97 != 0 {
                        continue;
                    }
                    let chunks = nexrad_data::aws::realtime::list_chunks_in_volume(site, v, 1)
                        .await
                        .unwrap();
                    if let Some(c) = chunks.first() {
                        assert!(
                            *c.date_time_prefix() <= started.naive_utc(),
                            "{site}/{probe} is newer"
                        );
                    }
                }
            }
        }
    }
}
