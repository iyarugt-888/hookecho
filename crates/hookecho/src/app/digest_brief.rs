//! Gathers the Storm Digest's evidence from what the app has computed for the active pane's
//! volume (see `crate::storm_brief` for what goes in and why warnings do not). Each detector is
//! read through its quiet, cached path — the digest never chimes, and a detector whose layer is
//! off is still run, so the brief does not depend on what happens to be drawn.

use super::*;
use crate::storm_brief::{self as sb, Brief, Signature, SignatureKind, StormBrief};

/// The most storms described; the rest are counted.
const MAX_STORMS: usize = 10;
/// The most signatures listed for one storm, and away from every storm.
const MAX_SIGNATURES: usize = 6;
const MAX_LOOSE: usize = 12;
/// A city label further than this from a storm does not name where it is.
const PLACE_KM: f64 = 60.0;
/// Trends look this far back from a cell's newest scan.
const TREND_MIN: i64 = 30;

impl HookEchoApp {
    /// Everything the app knows about the storms in the active pane's view.
    pub(crate) fn storm_brief(&mut self, ctx: &egui::Context) -> Brief {
        let idx = self.active;
        let (w, s, e, n) = self.view_bounds();
        let inside = |lon: f64, lat: f64| lon >= w && lon <= e && lat >= s && lat <= n;
        let mut b = Brief {
            site: self.views[idx].site.clone(),
            ..Default::default()
        };
        let radar = b
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| [s.longitude as f64, s.latitude as f64]);

        // The volume's own description, and the detectors run on it.
        let mut couplets = Vec::new();
        let mut debris = Vec::new();
        let mut tbss = Vec::new();
        let mut zdr = Vec::new();
        if let Some(v) = self.views[idx].volume.as_ref() {
            b.scan = Some(v.time);
            b.vcp = (!v.vcp.is_empty()).then(|| v.vcp.clone());
            b.tilt_deg = v.elevations.get(self.views[idx].tilt).copied();
            b.has_velocity = v.moments[Moment::Velocity.index()];
            b.has_dualpol = v.moments[Moment::CorrelationCoefficient.index()];
        }
        if self.views[idx].volume.is_some() {
            let (raw, _, scanned) = self.couplets_raw(idx);
            couplets = raw;
            debris = self.tds_quiet(idx);
            wxdata::tds::cross_corroborate(&mut debris, &mut couplets, scanned > 0);
            let d = &self.settings.detectors;
            let (rot_min, tds_min) = (d.rotation_min_confidence, d.tds_min_confidence);
            couplets.retain(|h| h.confidence >= rot_min);
            debris.retain(|h| h.confidence >= tds_min);
            tbss = self.compute_tbss(idx);
            zdr = self.compute_zdr_columns(idx, ctx);
            b.bright_band = self
                .zdr_cache
                .as_ref()
                .and_then(|c| c.2)
                .map(|bb| (bb.height_km, bb.mean_cc));
        }
        match self.freezing_for(idx) {
            Some((h0, hm20)) => {
                b.freezing_m = Some(h0);
                b.minus20_m = Some(hm20);
            }
            None if b.has_dualpol => b.notes.push(
                "ZDR columns not checked: the freezing level for this scan is not loaded yet (generate again in a moment)"
                    .into(),
            ),
            None => {}
        }
        if self.archive_bucket().is_some() {
            b.notes.push(
                "Archived scan: the SCIT storm-cell table is live-only, so storms are described by their detector signatures alone"
                    .into(),
            );
        }

        // Every signature in view, in words.
        let mut sigs: Vec<([f64; 2], SignatureKind, String, Option<f32>)> = Vec::new();
        sigs.extend(couplets.iter().map(|h| {
            let c = Some(h.confidence);
            ([h.lon, h.lat], SignatureKind::Rotation, sb::rotation(h), c)
        }));
        sigs.extend(debris.iter().map(|h| {
            let c = Some(h.confidence);
            ([h.lon, h.lat], SignatureKind::Debris, sb::debris(h), c)
        }));
        sigs.extend(
            tbss.iter()
                .map(|h| ([h.lon, h.lat], SignatureKind::HailSpike, sb::hail_spike(h), None)),
        );
        sigs.extend(
            zdr.iter()
                .map(|h| ([h.lon, h.lat], SignatureKind::ZdrColumn, sb::zdr_column(h), None)),
        );
        sigs.retain(|(p, ..)| inside(p[0], p[1]));

        // The storms in view, scored the way the Storms table scores them.
        let all = self.active_storm_cells().to_vec();
        let cells: Vec<wxdata::level3::Cell> =
            all.iter().filter(|c| inside(c.lon, c.lat)).cloned().collect();
        b.storms_elsewhere = all.len() - cells.len();
        let explained = wxdata::cellscore::score_all_explained(&cells, &self.probsevere, &couplets);
        let mut order: Vec<usize> = (0..cells.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(explained[i].score));
        order.truncate(MAX_STORMS);
        let centres: Vec<(f64, f64)> = order.iter().map(|&i| (cells[i].lon, cells[i].lat)).collect();
        let cores = self.core_rows_at(idx, &centres);
        let towns = self.town_labels();
        let flashes = self.recent_flashes();
        b.flashes_5min = flashes
            .as_ref()
            .map(|f| f.iter().filter(|p| inside(p[0], p[1])).count());

        for (k, &i) in order.iter().enumerate() {
            let c = &cells[i];
            let at = [c.lon, c.lat];
            let x = &explained[i];
            b.storms.push(StormBrief {
                id: if c.id.is_empty() {
                    c.title.clone()
                } else {
                    c.id.clone()
                },
                standalone: c.id.is_empty(),
                lon: c.lon,
                lat: c.lat,
                place: place_name(&towns, at),
                from_radar: radar.map(|r| crate::geo::great_circle(r, at)),
                score: Some(x.score),
                score_reasons: x
                    .reasons
                    .iter()
                    .map(|r| format!("{}: {}", r.label, r.detail))
                    .collect(),
                max_dbz: c.max_dbz,
                max_dbz_hgt_kft: c.max_dbz_hgt_kft,
                top_kft: c.top_kft,
                base_kft: c.base_kft,
                vil: c.vil,
                poh: c.poh,
                posh: c.posh,
                hail_in: c.hail_in,
                tvs: c.tvs.as_ref().is_some_and(|t| !t.is_empty()),
                meso: c.meso.as_ref().is_some_and(|m| !m.is_empty()),
                motion: c.mvt_deg.zip(c.mvt_kt),
                prob_severe: wxdata::overlay::hit(&self.probsevere, c.lon, c.lat)
                    .and_then(wxdata::cellscore::dominant_pct),
                core: cores
                    .get(k)
                    .map(|rows| rows.iter().filter(|(l, _)| l != "Gates").cloned().collect())
                    .unwrap_or_default(),
                signatures: Vec::new(),
                trend: sb::trend_of(&self.storm_trend(&c.id), TREND_MIN),
                flashes_5min: flashes.as_ref().map(|f| {
                    f.iter()
                        .filter(|p| crate::geo::great_circle(at, **p).0 <= sb::ATTACH_KM)
                        .count()
                }),
            });
        }

        // Each signature joins the nearest described storm core, or stands on its own.
        for (p, kind, detail, confidence) in sigs {
            let nearest = b
                .storms
                .iter()
                .enumerate()
                .map(|(i, st)| (i, crate::geo::great_circle([st.lon, st.lat], p)))
                .min_by(|a, b| a.1 .0.total_cmp(&b.1 .0));
            match nearest {
                Some((i, (km, bearing))) if km <= sb::ATTACH_KM => {
                    b.storms[i].signatures.push(Signature {
                        kind,
                        detail,
                        km,
                        bearing,
                        confidence,
                    })
                }
                _ => {
                    let (km, bearing) = radar.map_or((0.0, 0.0), |r| crate::geo::great_circle(r, p));
                    b.loose.push(Signature {
                        kind,
                        detail,
                        km,
                        bearing,
                        confidence,
                    });
                }
            }
        }
        let strongest_first = |v: &mut Vec<Signature>, max: usize| {
            v.sort_by(|a, b| {
                b.confidence
                    .unwrap_or(0.0)
                    .total_cmp(&a.confidence.unwrap_or(0.0))
                    .then(a.km.total_cmp(&b.km))
            });
            v.truncate(max);
        };
        for st in &mut b.storms {
            strongest_first(&mut st.signatures, MAX_SIGNATURES);
        }
        strongest_first(&mut b.loose, MAX_LOOSE);
        b
    }

    /// The town labels currently on the map, as `(name, lon/lat)`.
    fn town_labels(&self) -> Vec<(String, [f64; 2])> {
        let Some((_, labels)) = &self.vlabel_cache else {
            return Vec::new();
        };
        labels
            .iter()
            .filter(|l| l.city)
            .map(|l| {
                let (lon, lat) =
                    crate::render::mercator::world_to_lonlat(l.world[0] as f64, l.world[1] as f64);
                (l.name.clone(), [lon, lat])
            })
            .collect()
    }

    /// GOES flash positions from the last five minutes, when the lightning layer is on and the
    /// view is live (the feed is live-only, so it says nothing about an archived scan).
    fn recent_flashes(&self) -> Option<Vec<[f64; 2]>> {
        if !self.show_glm || self.archive_bucket().is_some() {
            return None;
        }
        let since = chrono::Utc::now() - chrono::Duration::minutes(5);
        let feed = self.glm.lock().ok()?;
        Some(
            feed.flashes()
                .iter()
                .filter(|f| f.time >= since)
                .map(|f| [f.lon, f.lat])
                .collect(),
        )
    }
}

/// "6 km SW of Moore" — the nearest town label within [`PLACE_KM`], or "in Moore" on top of it.
fn place_name(towns: &[(String, [f64; 2])], at: [f64; 2]) -> Option<String> {
    let (name, (km, bearing)) = towns
        .iter()
        .map(|(n, p)| (n, crate::geo::great_circle(*p, at)))
        .filter(|(_, (km, _))| *km <= PLACE_KM)
        .min_by(|a, b| a.1 .0.total_cmp(&b.1 .0))?;
    Some(if km < 2.0 {
        format!("in {name}")
    } else {
        format!("{km:.0} km {} of {name}", sb::compass(bearing))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_storm_is_named_from_the_nearest_town_within_reach() {
        let towns = vec![
            ("Moore".to_string(), [-97.486, 35.339]),
            ("Norman".to_string(), [-97.439, 35.222]),
        ];
        assert_eq!(
            place_name(&towns, [-97.486, 35.340]).as_deref(),
            Some("in Moore")
        );
        let sw = place_name(&towns, [-97.55, 35.30]).unwrap();
        assert!(sw.ends_with("SW of Moore"), "{sw}");
        assert!(place_name(&towns, [-99.5, 36.5]).is_none());
    }
}
