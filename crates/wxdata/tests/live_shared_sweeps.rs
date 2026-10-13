//! Live snapshots share immutable buffers without changing clocks, values or worker wire data.
use nexrad_model::data::{
    MomentData, PulseWidth, Radial, RadialStatus, Sweep, VolumeCoveragePattern,
};
use wxdata::{level2::Scan, live::merge_scan};

fn sweep(number: u8, timestamp: i64, value: u8) -> Sweep {
    Sweep::new(
        number,
        vec![Radial::new(
            timestamp,
            1,
            0.5,
            0.5,
            RadialStatus::ScanStart,
            number,
            number as f32,
            Some(MomentData::from_fixed_point(
                1,
                2125,
                250,
                8,
                2.0,
                66.0,
                vec![value; 1000],
            )),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .with_doppler_metadata(2500, 1200)],
    )
}

fn scan(sweeps: Vec<Sweep>) -> Scan {
    let vcp = VolumeCoveragePattern::new(
        212,
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
        vec![],
    );
    Scan::new(vcp, sweeps)
}

#[test]
fn changed_cut_detaches_while_unchanged_cuts_and_snapshots_share() {
    let base = scan(vec![sweep(1, 1000, 100), sweep(2, 1000, 100)]);
    let frozen = base.clone();
    let (updated, changed) = merge_scan(&base, scan(vec![sweep(1, 2000, 150)]));
    assert_eq!(changed, vec![1.0]);
    assert_ne!(
        base.sweeps()[0].radials().as_ptr(),
        updated.sweeps()[0].radials().as_ptr()
    );
    assert_eq!(
        base.sweeps()[1].radials().as_ptr(),
        updated.sweeps()[1].radials().as_ptr()
    );
    assert_eq!(
        base.sweeps()[0].radials().as_ptr(),
        frozen.sweeps()[0].radials().as_ptr()
    );
    assert_eq!(base, frozen);
    assert_eq!(base.sweeps()[0].radials()[0].collection_timestamp(), 1000);
    assert_eq!(
        updated.sweeps()[0].radials()[0].collection_timestamp(),
        2000
    );
    assert_eq!(
        updated.sweeps()[0].radials()[0].nyquist_velocity_mps(),
        Some(25.0)
    );
    drop(base);
    assert_eq!(frozen.sweeps()[1], updated.sweeps()[1]);
}

#[test]
fn empty_duplicate_and_late_updates_keep_the_existing_buffers() {
    let base = scan(vec![sweep(1, 2000, 150), Sweep::new(2, vec![])]);
    for partial in [scan(vec![]), base.clone(), scan(vec![sweep(1, 1000, 100)])] {
        let (updated, changed) = merge_scan(&base, partial);
        assert!(changed.is_empty());
        assert_eq!(updated, base);
        assert_eq!(
            updated.sweeps()[0].radials().as_ptr(),
            base.sweeps()[0].radials().as_ptr()
        );
    }
}

#[test]
fn consuming_sweep_merge_does_not_mutate_a_published_snapshot() {
    let first = sweep(1, 0, 100); // unknown source clock stays unknown
    let frozen = first.clone();
    let merged = first.merge(sweep(1, 2000, 150)).unwrap();
    assert_eq!(frozen.radials().len(), 1);
    assert_eq!(frozen.radials()[0].collection_timestamp(), 0);
    assert_eq!(merged.radials().len(), 2);
    assert_ne!(merged.radials().as_ptr(), frozen.radials().as_ptr());
    assert!(frozen.merge(Sweep::new(2, vec![])).is_err());
}

#[test]
fn shared_storage_preserves_json_and_postcard_radial_sequences() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct LegacySweep {
        elevation_number: u8,
        radials: Vec<Radial>,
    }
    for original in [sweep(1, 0, 100), Sweep::new(2, vec![])] {
        let legacy = LegacySweep {
            elevation_number: original.elevation_number(),
            radials: original.radials().to_vec(),
        };
        assert_eq!(
            serde_json::to_value(&original).unwrap(),
            serde_json::to_value(&legacy).unwrap()
        );
        let wire = postcard::to_allocvec(&legacy).unwrap();
        assert_eq!(wire, postcard::to_allocvec(&original).unwrap());
        let decoded: Sweep = postcard::from_bytes(&wire).unwrap();
        assert_eq!(decoded, original);
        let old_reader: LegacySweep =
            postcard::from_bytes(&postcard::to_allocvec(&original).unwrap()).unwrap();
        assert_eq!(old_reader.radials, original.radials());
    }
}

#[test]
#[ignore = "release performance measurement using the local scientific corpus"]
fn corpus_snapshot_copy_cost() {
    use std::{hint::black_box, time::Instant};
    let path = std::env::var_os("HOOKECHO_LIVE_BENCH_VOLUME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/scientific-corpus/KTLX20130520_201229_V06.gz")
        });
    let base = wxdata::level2::decode_volume(std::fs::read(&path).unwrap()).unwrap();
    assert!(base.sweeps().len() > 1);
    let iterations = 100;
    let measure = |deep: bool| {
        let mut times = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            let copy = if deep {
                Scan::new(
                    base.coverage_pattern().clone(),
                    base.sweeps()
                        .iter()
                        .map(|s| Sweep::new(s.elevation_number(), s.radials().to_vec()))
                        .collect(),
                )
            } else {
                base.clone()
            };
            black_box(&copy);
            times.push(start.elapsed().as_nanos());
            assert_eq!(copy.sweeps(), base.sweeps());
        }
        times.sort_unstable();
        (times[iterations / 2], times[iterations * 95 / 100])
    };
    // Alternate warm-up before timing either path; exclude decoding, equality and destruction.
    black_box(base.clone());
    black_box(
        base.sweeps()
            .iter()
            .map(|s| s.radials().to_vec())
            .collect::<Vec<_>>(),
    );
    let deep = measure(true);
    let shared = measure(false);
    println!("fixture={} sweeps={} radials={} iterations={} deep_copy_ns_p50={} p95={} shared_copy_ns_p50={} p95={}",
        path.display(), base.sweeps().len(), base.sweeps().iter().map(|s| s.radials().len()).sum::<usize>(),
        iterations, deep.0, deep.1, shared.0, shared.1);
}
