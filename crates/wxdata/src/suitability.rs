//! Radar suitability: which nearby radars can actually see a point, with the beam geometry to
//! show why, rather than a bare distance-sorted list.
//!
//! Prompted by a review of a real case (December 2021 western Kentucky): a storm's early track
//! sat well within a farther radar's low-level coverage while the nearest site's own beam was
//! already noticeably higher there — the kind of thing a distance-only "nearest site" list hides
//! entirely, and the numbers here (beam height and width at the target) make legible instead.
//!
//! One honest limitation worth stating up front: for a single `elevation_deg` applied uniformly,
//! beam height at a ground point is a strictly increasing function of distance — so [`rank`]'s
//! order is provably identical to sorting by distance alone (see this module's own tests). What
//! this adds over a plain distance list is not a different *order*, it's the beam height and
//! width numbers themselves: the two radars in the case above were both "nearby" by distance, and
//! only the beam geometry actually said which one still had eyes on the ground there. A ranking
//! that genuinely reorders vs. distance would need to know each candidate's real lowest achievable
//! elevation (which can differ by network and by VCP) rather than assuming one angle for every
//! site — not implemented here rather than guessed at with an unverified number.
//!
//! Pure geometry, computed from the static site registry — no network access, no live data. What
//! it deliberately does *not* attempt: newest-volume age, terrain blockage, or per-site product
//! availability, all of which need a live poll of each candidate rather than a lookup table. Those
//! stay real gaps, not silently assumed away — see `RadarCandidate`'s own doc comment.

use crate::sites::SiteEntry;

// The only antenna-shape assumption this module makes for every candidate — see
// `crate::beam_geometry::WSR88D_BEAMWIDTH_DEG`'s own doc comment for why that is a real gap for
// a TDWR/DWD/OPERA candidate in the ranking, not one this module claims to have closed.
use crate::beam_geometry::horizontal_beam_width_km;

/// One candidate radar's geometry at a target point.
#[derive(Debug, Clone, Copy)]
pub struct RadarCandidate {
    pub site: &'static SiteEntry,
    pub distance_km: f64,
    /// Beam-centre height above the antenna at the target, for the elevation angle [`rank`] was
    /// asked to score — the number that actually decides whether this radar sees low-level
    /// structure there or is already looking clean over the top of it. Not adjusted for the
    /// site's own ground elevation (AGL vs the radar's own base, not AGL at the target point,
    /// which would need terrain data this module doesn't have) or MSL.
    pub beam_height_m: f64,
    /// Approximate horizontal beam width at `distance_km`, from the small-angle approximation
    /// `distance_km * beamwidth_rad` — accurate to well under 1% at NEXRAD's beamwidth and normal
    /// operating ranges, not a claim of higher precision than that.
    pub beam_width_km: f64,
}

/// `site`'s beam geometry at `(lon, lat)` for `elevation_deg` — the one computation [`rank`] runs
/// for every candidate, pulled out so a caller who already knows which two (or more) sites it
/// wants (see [`crate::sites::SiteEntry`]) doesn't need [`rank`]'s own nearest-neighbor scan and
/// truncation just to ask about a specific site.
///
/// `elevation_deg` is normally the lowest tilt a site actually scans (0.5° for a WSR-88D
/// precipitation VCP); pass whatever the caller's active elevation is to score the tilt actually
/// being looked at rather than always assuming the base scan.
pub fn candidate_at(
    site: &'static crate::sites::SiteEntry,
    lon: f64,
    lat: f64,
    elevation_deg: f64,
) -> RadarCandidate {
    let (distance_km, _bearing_deg) =
        crate::xsection::dist_bearing(site.longitude as f64, site.latitude as f64, lon, lat);
    let slant_km = crate::xsection::slant_from_ground_km(distance_km, elevation_deg);
    let beam_height_m = crate::xsection::beam_height_km(slant_km, elevation_deg) * 1_000.0;
    let beam_width_km = horizontal_beam_width_km(distance_km);
    RadarCandidate {
        site,
        distance_km,
        beam_height_m,
        beam_width_km,
    }
}

/// The nearest `limit` radars to `(lon, lat)`, ranked by beam height at the target for
/// `elevation_deg` (ascending — lowest beam first) rather than by raw distance. Ties (there
/// usually aren't any at this precision) fall back to distance.
///
/// `elevation_deg` is normally the lowest tilt a site actually scans (0.5° for a WSR-88D
/// precipitation VCP); pass whatever the caller's active elevation is to rank for the tilt
/// actually being looked at rather than always assuming the base scan.
pub fn rank(lon: f64, lat: f64, elevation_deg: f64, limit: usize) -> Vec<RadarCandidate> {
    let mut candidates: Vec<RadarCandidate> = crate::sites::all()
        .map(|site| candidate_at(site, lon, lat, elevation_deg))
        .collect();
    candidates.sort_by(|a, b| {
        a.beam_height_m
            .total_cmp(&b.beam_height_m)
            .then_with(|| a.distance_km.total_cmp(&b.distance_km))
    });
    candidates.truncate(limit);
    candidates
}

/// How much lower another radar's beam must be before it is worth suggesting (1010.md V8): at
/// most this fraction of the current radar's height at the storm...
pub const RECOMMEND_RATIO: f64 = 0.6;
/// ...and at least this many metres lower. Near a radar every beam is low, and a few hundred
/// metres is not worth a site switch and a new storm table.
pub const RECOMMEND_MIN_GAIN_M: f64 = 600.0;

/// A radar worth switching to for a storm, and the geometry that says why.
#[derive(Debug, Clone, Copy)]
pub struct Recommendation {
    /// The radar to switch to, with its beam at the storm.
    pub better: RadarCandidate,
    /// The radar being watched now, with its beam at the storm.
    pub current: RadarCandidate,
}

/// The radar of `current`'s own network whose beam at `(lon, lat)` is lowest, when it is
/// meaningfully lower than `current`'s ([`RECOMMEND_RATIO`], [`RECOMMEND_MIN_GAIN_M`]). `None`
/// when `current` is already the best or close enough, or is not a known site.
///
/// Same network only: a TDWR or a European radar carries different products and a different
/// range, so it is not a drop-in replacement for the radar being watched. Like [`rank`], this is
/// geometry at one assumed elevation, so in practice it suggests a nearer radar; what it adds is
/// the beam heights that say whether the difference matters. Volume age and terrain blockage are
/// not known here (see the module doc), so the suggestion never claims a fresher radar.
pub fn recommend(current: &str, lon: f64, lat: f64, elevation_deg: f64) -> Option<Recommendation> {
    let current_site = crate::sites::site_by_id(current)?;
    let network = crate::sites::network(current_site.id);
    let now = candidate_at(current_site, lon, lat, elevation_deg);
    let better = crate::sites::all()
        .filter(|s| s.id != current_site.id && crate::sites::network(s.id) == network)
        .map(|s| candidate_at(s, lon, lat, elevation_deg))
        .min_by(|a, b| {
            a.beam_height_m
                .total_cmp(&b.beam_height_m)
                .then_with(|| a.distance_km.total_cmp(&b.distance_km))
        })?;
    worth_switching(now.beam_height_m, better.beam_height_m).then_some(Recommendation {
        better,
        current: now,
    })
}

/// Whether a beam `better_m` high at the storm is worth leaving one `current_m` high for.
pub fn worth_switching(current_m: f64, better_m: f64) -> bool {
    better_m <= current_m * RECOMMEND_RATIO && current_m - better_m >= RECOMMEND_MIN_GAIN_M
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mayfield, Kentucky, on 10 December 2021: Paducah is about 40 km away, Fort Campbell about
    /// 120 km. Watching from Fort Campbell, Paducah is suggested; watching from Paducah, nothing.
    #[test]
    fn a_much_nearer_radar_is_recommended_and_the_nearest_is_left_alone() {
        let (lon, lat) = (-88.636, 36.742);
        let r = recommend("KHPX", lon, lat, 0.5).expect("Paducah sees Mayfield far lower");
        assert_eq!(r.better.site.id, "KPAH");
        assert_eq!(r.current.site.id, "KHPX");
        assert!(r.better.beam_height_m < r.current.beam_height_m * RECOMMEND_RATIO);
        assert!(recommend("KPAH", lon, lat, 0.5).is_none());
    }

    /// Both radars already see low, or the other is only a little lower: not worth a switch.
    #[test]
    fn only_a_large_gain_is_worth_switching() {
        assert!(
            worth_switching(2_400.0, 900.0),
            "1.5 km lower and well under 60%"
        );
        assert!(
            !worth_switching(500.0, 100.0),
            "both low: 400 m is not worth a new radar"
        );
        assert!(
            !worth_switching(2_000.0, 1_300.0),
            "65% of the height: too close to call"
        );
        assert!(!worth_switching(900.0, 900.0));
    }

    /// Watching Oklahoma City from Tulsa: KTLX is suggested, never the TDWR beside it.
    #[test]
    fn only_a_radar_of_the_same_network_is_recommended() {
        let tokc = crate::tdwr::site_by_id("TOKC").unwrap();
        let (lon, lat) = (tokc.longitude as f64, tokc.latitude as f64);
        let r = recommend("KINX", lon, lat, 0.5).expect("KTLX is far nearer than Tulsa");
        assert_eq!(r.better.site.id, "KTLX");
        assert_eq!(
            crate::sites::network(r.better.site.id),
            crate::sites::Network::Nexrad
        );
        assert!(
            recommend("XXXX", lon, lat, 0.5).is_none(),
            "an unknown site has no beam"
        );
    }

    #[test]
    fn the_radar_at_the_target_itself_has_zero_distance_and_the_lowest_beam() {
        // KTLX's own coordinates: distance zero, beam height at range zero is also zero — no
        // other real site can beat "you are standing on the antenna".
        let ktlx = crate::sites::site_by_id("KTLX").unwrap();
        let ranked = rank(ktlx.longitude as f64, ktlx.latitude as f64, 0.5, 5);
        assert_eq!(ranked[0].site.id, "KTLX");
        assert!(ranked[0].distance_km < 0.01, "{}", ranked[0].distance_km);
        assert!(ranked[0].beam_height_m < 1.0, "{}", ranked[0].beam_height_m);
    }

    /// `candidate_at` is what `rank` calls per candidate — the two must agree on the same site
    /// at the same point, or the refactor that split them changed behavior by accident.
    #[test]
    fn candidate_at_agrees_with_rank_for_the_same_site() {
        let ktlx = crate::sites::site_by_id("KTLX").unwrap();
        let (lon, lat) = (-97.28, 35.33);
        let from_rank = rank(lon, lat, 0.5, 30)
            .into_iter()
            .find(|c| c.site.id == "KTLX")
            .expect("KTLX in range");
        let direct = candidate_at(ktlx, lon, lat, 0.5);
        assert_eq!(direct.site.id, from_rank.site.id);
        assert!((direct.distance_km - from_rank.distance_km).abs() < 1e-9);
        assert!((direct.beam_height_m - from_rank.beam_height_m).abs() < 1e-9);
        assert!((direct.beam_width_km - from_rank.beam_width_km).abs() < 1e-9);
    }

    #[test]
    fn limit_caps_the_result_and_results_are_sorted_by_beam_height() {
        let ranked = rank(-97.28, 35.33, 0.5, 8);
        assert_eq!(ranked.len(), 8);
        assert!(
            ranked
                .windows(2)
                .all(|w| w[0].beam_height_m <= w[1].beam_height_m),
            "{:?}",
            ranked.iter().map(|c| c.beam_height_m).collect::<Vec<_>>()
        );
    }

    /// Documents the module doc comment's own honest limitation: beam height is a strictly
    /// increasing function of ground distance for one fixed elevation, so today's ranking is
    /// provably distance order — not a claim this module doesn't make, locked in so a future
    /// change either keeps it true or updates the doc comment that says so.
    #[test]
    fn ranking_matches_distance_order_for_one_fixed_elevation() {
        let ranked = rank(-97.28, 35.33, 0.5, 30);
        let distances: Vec<f64> = ranked.iter().map(|c| c.distance_km).collect();
        assert!(distances.windows(2).all(|w| w[0] <= w[1]), "{distances:?}");
    }

    #[test]
    fn beam_width_and_beam_height_both_grow_with_range_for_the_same_site() {
        let ktlx = crate::sites::site_by_id("KTLX").unwrap();
        let (klon, klat) = (ktlx.longitude as f64, ktlx.latitude as f64);
        // Two points due east of KTLX at different distances — find KTLX's own row in a big
        // enough candidate list at each, rather than assuming it ranks first (it needn't, once
        // the target is far enough that a closer site's beam is lower there).
        let at = |km: f64| -> RadarCandidate {
            let (lon, lat) =
                crate::beam_geometry::destination_lonlat(klon, klat, 90.0, km * 1_000.0);
            rank(lon, lat, 0.5, 200)
                .into_iter()
                .find(|c| c.site.id == "KTLX")
                .expect("KTLX must appear somewhere in a 200-candidate ranking of its own targets")
        };
        let near = at(40.0);
        let far = at(160.0);
        assert!(
            far.beam_width_km > near.beam_width_km,
            "{far:?} vs {near:?}"
        );
        assert!(
            far.beam_height_m > near.beam_height_m,
            "{far:?} vs {near:?}"
        );
    }
}
