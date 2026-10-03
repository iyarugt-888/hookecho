//! A pane's detections for this frame: the nowcast points, the signature detectors (debris,
//! hail spikes, ZDR columns, couplets), their score tracks, local cell tracks, and the merged
//! one-per-tornado detections, each computed when its layer or an armed rule wants it and then
//! filtered to the layers shown. Moved out of `render_pane` unchanged (ROADMAP_2 §7).

use super::*;

/// What [`HookEchoApp::pane_detections`] found, under the names `render_pane` draws them by.
pub(crate) struct PaneDetections {
    pub nowcast_pts: Vec<(f64, f64, egui::Color32)>,
    pub tds_hits: Vec<wxdata::tds::TdsHit>,
    pub tds_score_tracks: Vec<wxdata::scoretrack::ScoreTrack>,
    pub tbss_hits: Vec<wxdata::dualpol::TbssHit>,
    pub zdr_hits: Vec<wxdata::dualpol::ZdrColumnHit>,
    pub couplets: Vec<wxdata::rotation::CoupletHit>,
    pub rot_score_tracks: Vec<wxdata::scoretrack::ScoreTrack>,
    pub local_tracks: Vec<wxdata::celltrack::Track>,
    pub tornado_ids: Vec<wxdata::tornado_id::TornadoId>,
    pub circulations: Vec<wxdata::tornado_id::Circulation>,
    /// Where this pane's Tornado ID verdicts came from, when it shows any.
    pub tornado_lineage: Option<wxdata::detection_lineage::DetectionLineage>,
    pub tied_couplet: Vec<bool>,
    pub tied_tds: Vec<bool>,
    pub all_couplets: Vec<wxdata::rotation::CoupletHit>,
    pub all_tds: Vec<wxdata::tds::TdsHit>,
    /// The experimental LLSD pipeline, analysed, when `detectors.llsd_preview` is on.
    pub llsd: Vec<wxdata::llsd_analyst::Analysed>,
}

impl HookEchoApp {
    pub(crate) fn pane_detections(&mut self, ctx: &egui::Context, idx: usize) -> PaneDetections {
        // Optical-flow nowcast points (needs &mut self to bin the sweep; done before the &view borrow).
        let nowcast_pts = if self.filters.show_nowcast && idx == self.active {
            self.compute_nowcast(idx)
        } else {
            Vec::new()
        };
        // A rule that watches a signature has to drive its detector even with the layer off —
        // otherwise arming "hail spike near home" and then hiding the layer silently disarms it.
        // The hits are computed here either way; only the drawing below checks the layer flag.
        let armed = |t: &crate::settings::RuleTrigger| {
            self.settings
                .alert_rules
                .iter()
                .any(|r| r.enabled && &r.trigger == t)
        };
        // The rotation detectors and Tornado ID are not run on a TDWR. Their thresholds and the
        // fusion's weights were fitted on WSR-88D data only. On O'Hare's TDWR through the
        // 2020-08-10 derecho, the fused Tornado ID would have shown 78 Possible markers in an
        // hour, 2 of them near a tornado report: its 150 m C-band gates read routinely higher
        // shear (detectionplan.md, TDWR feasibility). A TDWR has no dual-pol, so there is no
        // debris evidence either.
        let tdwr = self.views[idx]
            .site
            .as_deref()
            .is_some_and(wxdata::tdwr::is_tdwr);
        // Tornado detection is one feature: the rotation and debris detectors feed Tornado ID, which
        // is drawn as one marker per tornado that opens into the web of the detections it ties
        // together (`wxdata::tornado_id::circulations`). A rule that watches rotation or debris
        // drives its detector even with the feature off.
        let tornado = self.filters.show_tornado_id;
        let want_tds = tornado || armed(&crate::settings::RuleTrigger::Tds);
        let want_tbss = self.filters.show_tbss || armed(&crate::settings::RuleTrigger::Tbss);
        let want_zdr =
            self.filters.show_zdr_columns || armed(&crate::settings::RuleTrigger::ZdrColumn);
        let want_couplets = !tdwr && (tornado || armed(&crate::settings::RuleTrigger::Rotation));
        let tds_hits = if want_tds && idx == self.active {
            self.compute_tds(idx)
        } else {
            Vec::new()
        };
        // Score history for the hover sparkline below — same "only for the active pane, and only
        // when the layer is actually wanted" gating as `tds_hits` itself, since it costs a replay
        // over `tds_shown_cache` even on a cache hit's cheap path.
        let tds_score_tracks = if want_tds && idx == self.active {
            self.compute_tds_score_track(idx)
        } else {
            Vec::new()
        };
        let tbss_hits = if want_tbss && idx == self.active {
            self.compute_tbss(idx)
        } else {
            Vec::new()
        };
        let zdr_hits = if want_zdr && idx == self.active {
            self.compute_zdr_columns(idx, ctx)
        } else {
            Vec::new()
        };
        let couplets = if want_couplets && idx == self.active {
            self.compute_couplets(idx)
        } else {
            Vec::new()
        };
        let rot_score_tracks = if want_couplets && idx == self.active {
            self.compute_rot_score_track(idx)
        } else {
            Vec::new()
        };
        let local_tracks = if self.show_local_tracks && idx == self.active {
            self.compute_local_tracks()
        } else {
            Vec::new()
        };
        if idx == self.active {
            self.check_rain_arrival();
            self.evaluate_scan_rules(idx, &tds_hits, &tbss_hits, &zdr_hits, &couplets);
        }
        // Tornado ID, from the fusion or the legacy detectors (`detectors.tornado_id_source`): every
        // rotation and debris detection near a tornado is part of that tornado's one marker, and
        // the full lists stay for the web it opens into.
        let (tornado_ids, circulations, tornado_lineage) = if idx == self.active && !tdwr && tornado
        {
            let (ids, circs, lineage) =
                self.tornado_identifications(idx, ctx, &couplets, &tds_hits, true);
            // For the local API and the analysis export: the verdicts, one per tornado.
            let shown = circs.iter().map(|c| c.id.clone()).collect();
            self.tornado_shown = Some((lineage.volume.clone(), shown, lineage.clone()));
            self.tornado_alert(idx, &circs, Some(&lineage));
            (ids, circs, Some(lineage))
        } else {
            (Vec::new(), Vec::new(), None)
        };
        let mut tied_couplet = vec![false; couplets.len()];
        let mut tied_tds = vec![false; tds_hits.len()];
        for c in &circulations {
            for m in &c.members {
                match m.evidence {
                    wxdata::tornado_id::Evidence::Rotation(i) => tied_couplet[i] = true,
                    wxdata::tornado_id::Evidence::Debris(i) => tied_tds[i] = true,
                }
            }
        }
        let (all_couplets, all_tds) = (couplets.clone(), tds_hits.clone());
        // Rotation and debris are drawn only as part of a tornado's marker and its web, never on
        // their own: one tied to no verdict is what the fusion weighed and set aside. (Hidden
        // layers computed only for a rule are not drawn either.)
        let tds_hits: Vec<wxdata::tds::TdsHit> = Vec::new();
        let tbss_hits = if self.filters.show_tbss {
            tbss_hits
        } else {
            Vec::new()
        };
        let zdr_hits = if self.filters.show_zdr_columns {
            zdr_hits
        } else {
            Vec::new()
        };
        let llsd = if self.settings.detectors.llsd_preview && idx == self.active && !tdwr {
            self.compute_llsd(idx, ctx).unwrap_or_default()
        } else {
            Vec::new()
        };
        let couplets: Vec<wxdata::rotation::CoupletHit> = Vec::new();
        PaneDetections {
            nowcast_pts,
            tds_hits,
            tds_score_tracks,
            tbss_hits,
            zdr_hits,
            couplets,
            rot_score_tracks,
            local_tracks,
            tornado_ids,
            circulations,
            tornado_lineage,
            tied_couplet,
            tied_tds,
            all_couplets,
            all_tds,
            llsd,
        }
    }
}
