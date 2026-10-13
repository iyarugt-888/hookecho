//! An archive's storm labels through a real replay (roadmaps/storm-analysis S1): KTLX on
//! 2013-05-20, 19:00–20:30 UTC, the Moore tornado's storm. The app rebuilds its archive storm
//! table from the last 16 decoded volumes at the playhead (`celltrack::replay`) and carries the
//! labels from one table to the next (`celltrack::LocalLabels`). Here the same pipeline runs over
//! the real volumes three ways an analyst produces: stepping forward through the hour (the window
//! slides once it is full), stepping back, and volumes finishing decoding newest first at one
//! playhead, as the prefetch delivers them. The Moore storm must keep one label throughout, and
//! no label may name two storms in one table.
//!
//!     cargo test -p wxdata --test archive_cell_labels -- --ignored --nocapture
//!
//! Network on first run; volumes are kept in `target/trail-volumes` (shared with the Moore trail
//! capture).

use std::collections::HashSet;
use wxdata::celltrack::{find_cells, replay, Blob, LocalLabels};
use wxdata::level2::{self, Moment};
use wxdata::level3::Cell;

/// The app's window (`detectors::sync_archive_cells`).
const WINDOW: usize = 16;
/// Near Moore at the last volume (20:29Z): the tornadic storm's core.
const MOORE: (f64, f64) = (-97.45, 35.32);

type Frame = (chrono::DateTime<chrono::Utc>, Vec<Blob>);

fn frames() -> Vec<Frame> {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = repo.join("target/trail-volumes");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
    let from = day.and_hms_opt(19, 0, 0).unwrap().and_utc();
    let to = day.and_hms_opt(20, 30, 0).unwrap().and_utc();
    let ids: Vec<_> = rt
        .block_on(level2::list_volumes("KTLX", day))
        .expect("archive listing")
        .into_iter()
        .filter(|id| id.date_time().is_some_and(|t| t >= from && t <= to))
        .filter(|id| !id.name().ends_with("_MDM"))
        .collect();
    ids.into_iter()
        .filter_map(|id| {
            let t = id.date_time()?;
            let scan = rt
                .block_on(level2::download_scan(id, Some(cache.clone())))
                .expect("archive volume");
            // As the app: the lowest tilt, 45 dBZ cores.
            let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).ok()?;
            Some((t, find_cells(&sweep, 45.0)))
        })
        .collect()
}

fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.1 - a.1) * 110.57;
    let dx = (b.0 - a.0) * 111.32 * ((a.1 + b.1) * 0.5).to_radians().cos();
    dx.hypot(dy)
}

fn on(p: &(f64, f64), q: (f64, f64)) -> bool {
    p.0.to_bits() == q.0.to_bits() && p.1.to_bits() == q.1.to_bits()
}

/// The labels of one table are distinct: no label names two storms.
fn distinct(table: &[Cell]) {
    let mut seen = HashSet::new();
    for c in table {
        assert!(seen.insert(c.id.clone()), "{} names two storms", c.id);
    }
}

/// The row in `table` that is the storm whose positions are `track` (its current position is one
/// of them), if any.
fn the_storm<'a>(table: &'a [Cell], track: &[(f64, f64)]) -> Option<&'a Cell> {
    table
        .iter()
        .find(|c| track.iter().any(|p| on(p, (c.lon, c.lat))))
}

#[test]
#[ignore = "network"]
fn the_moore_storm_keeps_one_label_through_a_replay() {
    let frames = frames();
    let n = frames.len();
    assert!(n > WINDOW + 2, "{n} volumes: the window must slide");
    let window = |k: usize| &frames[(k + 1).saturating_sub(WINDOW)..=k];

    // Forward through the hour and a half, one volume a step.
    let mut labels = LocalLabels::default();
    let mut forward: Vec<Vec<Cell>> = Vec::new();
    let mut raw: Vec<Vec<Cell>> = Vec::new();
    for k in 0..n {
        let mut table = replay(window(k), "KTLX");
        raw.push(table.clone());
        labels.relabel(forward.last().map_or(&[][..], Vec::as_slice), &mut table);
        distinct(&table);
        forward.push(table);
    }
    let last = forward.last().unwrap();
    let moore = last
        .iter()
        .filter(|c| km((c.lon, c.lat), MOORE) < 15.0)
        .max_by(|a, b| a.max_dbz.partial_cmp(&b.max_dbz).unwrap())
        .expect("the Moore storm at 20:29Z");
    // Its row at each earlier step, found through the track of the row after it (the tracker's
    // own link), as far back as the tracker links it.
    let mut steps = Vec::new();
    let mut track = moore.past_track.clone();
    for k in (0..n).rev() {
        let Some(c) = the_storm(&forward[k], &track) else {
            continue;
        };
        let raw_id = the_storm(&raw[k], &track).map(|r| r.id.clone());
        steps.push((k, frames[k].0, c.id.clone(), raw_id));
        track = c.past_track.clone();
    }
    steps.reverse();
    for (k, t, id, raw_id) in &steps {
        println!(
            "step {k:2} {} label {id:6} replay's own {raw_id:?}",
            t.format("%H:%MZ")
        );
    }
    let span = steps.last().unwrap().1 - steps[0].1;
    println!(
        "followed over {} min, {} tables",
        span.num_minutes(),
        steps.len()
    );
    // Before that the tracker holds no link to it (its association, not the labels: this lane does
    // not tune it). For the record, what sat near its first linked position the volume before.
    let (k0, ..) = steps[0];
    let first = the_storm(&forward[k0], &moore.past_track).map(|c| (c.lon, c.lat));
    if let (Some(p), Some(prev)) = (first, k0.checked_sub(1).map(|k| &forward[k])) {
        for c in prev.iter().filter(|c| km((c.lon, c.lat), p) < 20.0) {
            println!(
                "step {:2} nearby: {} at {:.1} km, {:?} dBZ, track of {}",
                k0 - 1,
                c.id,
                km((c.lon, c.lat), p),
                c.max_dbz,
                c.past_track.len()
            );
        }
    }
    assert!(
        span.num_minutes() >= 25 && steps.len() >= 6,
        "the storm is followed for as long as the tracker links it"
    );
    assert!(
        steps.iter().all(|(.., id, _)| *id == moore.id),
        "one label throughout: {steps:?}"
    );
    let raw_ids: HashSet<_> = steps.iter().filter_map(|s| s.3.clone()).collect();
    println!("replay's own IDs for the storm over the steps: {raw_ids:?}");

    // Back again, sharing a volume with the last table each step.
    let mut back = forward.last().unwrap().clone();
    for k in (0..n - 1).rev() {
        let mut table = replay(window(k), "KTLX");
        labels.relabel(&back, &mut table);
        distinct(&table);
        if let Some(c) = the_storm(&table, &moore.past_track) {
            assert_eq!(c.id, moore.id, "backward at step {k}");
        }
        back = table;
    }

    // At the last playhead, the window's volumes finishing decoding newest first.
    let full = window(n - 1);
    let mut labels = LocalLabels::default();
    let mut table: Vec<Cell> = Vec::new();
    let mut first_label = None;
    for have in 1..=full.len() {
        let mut next = replay(&full[full.len() - have..], "KTLX");
        labels.relabel(&table, &mut next);
        distinct(&next);
        let c = the_storm(&next, &moore.past_track).expect("the storm is in its own volume");
        let label = first_label.get_or_insert_with(|| c.id.clone());
        assert_eq!(&c.id, label, "with {have} volumes decoded");
        table = next;
    }
}
