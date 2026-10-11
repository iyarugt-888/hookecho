//! `if_missing` on a real volume (1010.md V17): the Moore tornado's KTLX volume at 20:12 UTC on
//! 2013-05-20, from the local scientific corpus (`target/scientific-corpus/`, not in git).
//!
//! KDP is derived only where the differential phase supports it, so across the lowest tilt it is
//! missing at many gates where reflectivity was recorded. `REF + KDP` is missing there; `REF +
//! if_missing(KDP, 0)` fills those gates and nothing else: wherever reflectivity itself was not
//! recorded it stays missing, so the fallback never draws beyond the radar's own echo.

use wxdata::level2::{bin_scan, decode_volume, BinnedSweep};
use wxdata::udp::parse;
use wxdata::udp_volume::{evaluate_tilt, pair_tilts, Env, MOMENTS};

#[test]
#[ignore = "reads the local scientific corpus (target/scientific-corpus)"]
fn a_fallback_fills_a_missing_input_on_a_real_volume_and_nothing_beyond_the_echo() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/scientific-corpus/KTLX20130520_201229_V06.gz");
    let scan = decode_volume(std::fs::read(&path).expect("the corpus volume")).unwrap();
    let per_moment: [Vec<Option<BinnedSweep>>; 6] =
        std::array::from_fn(|i| vec![bin_scan(&scan, MOMENTS[i], 0).ok()]);
    let tilts = pair_tilts(&per_moment);
    let tilt = &tilts[0];
    let run = |src: &str| {
        evaluate_tilt(&parse(src).unwrap(), tilt, Env::default())
            .expect("the tilt has the moments")
            .1
    };
    let plain = run("REF + KDP");
    let filled = run("REF + if_missing(KDP, 0)");
    let reflectivity = run("REF");
    let count = |v: &[Option<f32>]| v.iter().filter(|x| x.is_some()).count();
    assert_eq!(plain.len(), filled.len());
    assert_eq!(filled.len(), reflectivity.len());
    for ((p, f), r) in plain.iter().zip(&filled).zip(&reflectivity) {
        // Where both were recorded the two agree; a value is filled only where REF was recorded,
        // and there it is REF itself when KDP is the missing one.
        if let Some(p) = p {
            assert_eq!(Some(*p), *f);
        }
        assert_eq!(f.is_some(), r.is_some());
        if p.is_none() {
            if let (Some(f), Some(r)) = (f, r) {
                assert_eq!(f, r);
            }
        }
    }
    let (n_plain, n_filled) = (count(&plain), count(&filled));
    println!(
        "KTLX 2013-05-20 20:12 UTC, {:.1} deg: {} gates; REF + KDP has {n_plain} values, \
         REF + if_missing(KDP, 0) has {n_filled}, REF alone {}",
        tilt[0].unwrap().elevation_deg,
        filled.len(),
        count(&reflectivity)
    );
    assert!(n_filled > n_plain, "{n_filled} vs {n_plain}");
}
