//! The Storms window (roadmap Q2's "denser product tables" and "dockable analyst panels"): every
//! SCIT cell in one compact table at dock width, ranked by the same severity score as the Storm
//! attributes window (`wxdata::cellscore`), so the two can never order the storms differently.
//!
//! A row click selects the storm — the map rings it and the Inspector's storm section shows it
//! (see `HookEchoApp::select_storm`), without switching this table's dock away; a double click
//! also centers the map on it. Headers sort; a
//! second click on the same header flips the direction. Rotation (TVS, meso) is a letter in the
//! last column, not a colour alone.

use super::*;
use crate::ui::a11y::Named as _;
use crate::ui::cells_window::{sorted_indices, SortCol};
use egui::{FontId, Rect, Sense, Stroke};
use egui_phosphor::regular as ph;

pub(super) const STORMS_W: f32 = 320.0;
const ROW_H: f32 = 22.0;

/// The table's columns: header, sort key, width.
const COLS: [(&str, SortCol, f32); 8] = [
    ("Sev", SortCol::Rank, 34.0),
    ("ID", SortCol::Id, 30.0),
    ("Rng", SortCol::Range, 34.0),
    ("dBZ", SortCol::MaxDbz, 32.0),
    ("Top", SortCol::Top, 30.0),
    ("VIL", SortCol::Vil, 30.0),
    ("PSH", SortCol::Posh, 34.0),
    ("Hail", SortCol::Hail, 34.0),
];

#[derive(Clone, Copy)]
enum StormAction {
    Select,
    Center,
    Details,
    Track,
}

/// Filter after sorting so the indices still address the source cells and their evidence.
fn matching_order(cells: &[wxdata::level3::Cell], order: Vec<usize>, query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    order
        .into_iter()
        .filter(|&i| cells[i].id.to_lowercase().contains(&query))
        .collect()
}

/// One cell's row text, in `COLS` order, then the rotation flags. Unknown values are a dash.
pub(super) fn row_cells(c: &wxdata::level3::Cell, score: Option<u8>) -> ([String; 8], String) {
    let o = |v: Option<f32>, p: usize| v.map_or_else(|| "\u{2014}".into(), |x| format!("{x:.p$}"));
    let cols = [
        score.map_or_else(|| "\u{2014}".into(), |s| s.to_string()),
        c.id.clone(),
        o(c.range_nm, 0),
        o(c.max_dbz, 0),
        o(c.top_kft, 0),
        o(c.vil, 0),
        c.posh
            .map_or_else(|| "\u{2014}".into(), |p| format!("{p}%")),
        c.hail_in
            .filter(|h| *h > 0.0)
            .map_or_else(|| "\u{2014}".into(), |h| format!("{h:.2}")),
    ];
    let mut flags = String::new();
    if c.tvs.as_ref().is_some_and(|t| !t.is_empty()) {
        flags.push('T');
    }
    if c.meso.as_ref().is_some_and(|m| !m.is_empty()) {
        flags.push('M');
    }
    (cols, flags)
}

/// A row's marks at its right end: the rotation flags in the warning colour, and to their left
/// the storm's split/merge lineage tag (M2.2), quieter.
fn paint_row_marks(p: &egui::Painter, r: Rect, flags: &str, lineage: Option<&str>, t: &ws::Tokens) {
    let mut left = r.right() - 8.0;
    if !flags.is_empty() {
        left = p
            .text(
                egui::pos2(left, r.center().y),
                egui::Align2::RIGHT_CENTER,
                flags,
                FontId::monospace(11.0),
                t.danger,
            )
            .left()
            - 6.0;
    }
    if let Some(tag) = lineage {
        p.text(
            egui::pos2(left, r.center().y),
            egui::Align2::RIGHT_CENTER,
            tag,
            FontId::proportional(10.0),
            t.text_dim,
        );
    }
}

/// The active radar's persistent storm history (ROADMAP_PARITY M2.1): fed once per SCIT table,
/// so each storm keeps one stable local ID across scans and provider-ID changes, with the evidence
/// for every link in [`wxdata::storm_history`].
#[derive(Debug, Default)]
pub(crate) struct StormIdentity {
    site: Option<String>,
    fed: Option<i64>,
    history: wxdata::storm_history::StormHistory,
    /// The storm of each SCIT cell in the table last fed, by cell ID, and where it was.
    current: Vec<(String, wxdata::storm_history::StormId, [f64; 2])>,
    /// What each storm has been linked to over time (warnings, ProbSevere, tornado detections,
    /// hail), recorded per SCIT scan beside the identity history.
    evidence: wxdata::storm_evidence::StormEvidence,
    /// The inputs the current scan's evidence was recorded from, so it is re-recorded (the same
    /// scan replaced, not added) when a late input arrives and not every frame.
    evidence_key: Option<EvidenceKey>,
}

/// Where a picked storm is in the current SCIT table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// In it, as this cell (which may carry a different SCIT ID than when it was picked).
    Current(String),
    /// Known, but not in the current table (missed, or no longer tracked): the cell that has its
    /// old ID now is another storm.
    Gone(wxdata::storm_history::StormId),
    /// The history has no record of the pick (another radar, an archive, a restarted history).
    Unknown,
}

/// What a scan's evidence depends on, cheaply: a change re-records that scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvidenceKey {
    pub scan: i64,
    pub volume: String,
    /// Which detector results for `volume` existed: rotation, debris, the fused analysis.
    pub detectors: [bool; 3],
    /// A hash of the warning and ProbSevere features' identities and text.
    pub features: u64,
}

fn features_hash(features: &[&[wxdata::overlay::GeoFeature]]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for list in features {
        list.len().hash(&mut h);
        for f in list.iter() {
            f.title.hash(&mut h);
            f.detail.len().hash(&mut h);
            f.alert.as_ref().map(|a| &a.id).hash(&mut h);
            f.rings.iter().map(Vec::len).sum::<usize>().hash(&mut h);
        }
    }
    h.finish()
}

impl StormIdentity {
    /// Feed the current SCIT table, once per scan time. Another radar starts a new history; no
    /// cells (an archive, where SCIT is not kept) feed nothing.
    pub(crate) fn feed(&mut self, site: Option<&str>, cells: &[wxdata::level3::Cell]) {
        use wxdata::storm_history::{ObservationRef, Source};
        if self.site.as_deref() != site {
            *self = StormIdentity {
                site: site.map(str::to_string),
                ..Default::default()
            };
        }
        let storms: Vec<&wxdata::level3::Cell> =
            cells.iter().filter(|c| !c.id.is_empty()).collect();
        let Some(time) = storms.iter().find_map(|c| c.time).map(|t| t.timestamp()) else {
            return;
        };
        if self.fed == Some(time) {
            return;
        }
        self.fed = Some(time);
        let site = site.unwrap_or_default().to_string();
        let observations: Vec<ObservationRef> = storms
            .iter()
            .map(|c| ObservationRef {
                source: Source::Scit,
                site: site.clone(),
                provider_id: Some(c.id.clone()),
                time,
                lon: c.lon,
                lat: c.lat,
                provider_motion_ms: scit_motion_ms(c),
            })
            .collect();
        let report = self.history.update(time, &observations);
        self.current = storms
            .iter()
            .zip(&report.storms)
            .map(|(c, s)| (c.id.clone(), *s, [c.lon, c.lat]))
            .collect();
    }

    /// Record the current scan's evidence, when its inputs differ from what it was last recorded
    /// from. A warning or ProbSevere polygon links every storm inside it; a fused tornado
    /// detection the storm nearest it ([`super::storm_associations::params`], the rule the Cell
    /// window shows), or every equally near storm as ambiguous; SCIT's hail attributes the cell's
    /// own storm.
    pub(crate) fn record_evidence(
        &mut self,
        key: EvidenceKey,
        cells: &[wxdata::level3::Cell],
        warnings: &[wxdata::overlay::GeoFeature],
        probsevere: &[wxdata::overlay::GeoFeature],
        circulations: &[wxdata::tornado_id::Circulation],
        volume_time: Option<i64>,
    ) {
        use wxdata::storm_evidence::{EvidenceKind, EvidenceObject, Shape};
        if self.fed != Some(key.scan) || self.evidence_key.as_ref() == Some(&key) {
            return;
        }
        let storms: Vec<(wxdata::storm_history::StormId, [f64; 2])> =
            self.current.iter().map(|(_, s, at)| (*s, *at)).collect();
        let mut objects: Vec<EvidenceObject> = Vec::new();
        for w in warnings
            .iter()
            .filter(|w| w.kind == wxdata::overlay::FeatureKind::Warning)
        {
            objects.push(EvidenceObject {
                kind: EvidenceKind::Warning,
                source_id: w
                    .alert
                    .as_ref()
                    .map_or_else(|| w.title.clone(), |a| a.event_key()),
                detail: w.title.clone(),
                // When it took effect, as the product says.
                valid: w
                    .alert
                    .as_ref()
                    .and_then(|a| a.effective.or(a.issued))
                    .map(|t| t.timestamp()),
                shape: Shape::Polygon(w.rings.clone()),
            });
        }
        for p in probsevere {
            objects.push(EvidenceObject {
                kind: EvidenceKind::ProbSevere,
                source_id: wxdata::probsevere::object_id(p)
                    .map_or_else(|| "(no ID)".to_string(), |id| format!("object {id}")),
                detail: p.title.clone(),
                valid: None,
                shape: Shape::Polygon(p.rings.clone()),
            });
        }
        // Strongest first: a storm with two detections in one scan keeps the stronger.
        let mut strongest: Vec<&wxdata::tornado_id::Circulation> = circulations.iter().collect();
        strongest.sort_by(|a, b| {
            b.id.tier
                .cmp(&a.id.tier)
                .then(b.id.score.total_cmp(&a.id.score))
        });
        for c in strongest {
            let mut detail = c.id.tier.label().to_string();
            if let Some(v) = c.id.vrot_ms {
                detail.push_str(&format!(", Vrot {v:.0} m/s"));
            }
            if let Some(cc) = c.id.min_cc {
                detail.push_str(&format!(", min CC {cc:.2}"));
            }
            objects.push(EvidenceObject {
                kind: EvidenceKind::TornadoDetection,
                // Detections are re-made each volume with no identity of their own: the record is
                // one track of them per storm, each sample saying which volume it came from.
                source_id: "Tornado ID".into(),
                detail,
                valid: volume_time,
                shape: Shape::Points(super::storm_associations::circulation_points(c)),
            });
        }
        for (i, (id, ..)) in self.current.iter().enumerate() {
            let Some(c) = cells.iter().find(|c| &c.id == id) else {
                continue;
            };
            let mut parts = Vec::new();
            if let Some(p) = c.posh {
                parts.push(format!("POSH {p}%"));
            }
            if let Some(p) = c.poh {
                parts.push(format!("POH {p}%"));
            }
            if let Some(h) = c.hail_in.filter(|h| *h > 0.0) {
                parts.push(format!("MEHS {h:.2} in"));
            }
            let any =
                c.posh.unwrap_or(0) > 0 || c.poh.unwrap_or(0) > 0 || c.hail_in.unwrap_or(0.0) > 0.0;
            if !any {
                continue;
            }
            objects.push(EvidenceObject {
                kind: EvidenceKind::Hail,
                source_id: "SCIT".into(),
                detail: format!("cell {id}: {}", parts.join(", ")),
                valid: c.time.map(|t| t.timestamp()),
                shape: Shape::Own(i),
            });
        }
        self.evidence.update(key.scan, &storms, &objects);
        self.evidence_key = Some(key);
    }

    /// The evidence lines for a cell's storm, most recently seen first.
    pub(crate) fn evidence_lines(
        &self,
        cell_id: &str,
        fmt_time: impl Fn(i64) -> String,
    ) -> Vec<String> {
        let Some(s) = self.storm_of(cell_id) else {
            return Vec::new();
        };
        self.evidence
            .of(s.id)
            .into_iter()
            .map(|t| wxdata::storm_evidence::describe(t, &fmt_time))
            .collect()
    }

    /// Where a storm picked from an earlier table (by its SCIT ID then, at that table's time) is
    /// now. SCIT recycles IDs and changes them, so the ID alone cannot say: the history can.
    pub(crate) fn resolve(&self, cell_id: &str, time: Option<i64>) -> Resolved {
        let Some(time) = time.filter(|_| !cell_id.is_empty()) else {
            return Resolved::Unknown;
        };
        let picked = self.history.storms().iter().find(|s| {
            s.observations
                .iter()
                .rev()
                .any(|o| o.time == time && o.provider_id.as_deref() == Some(cell_id))
        });
        let Some(storm) = picked else {
            return Resolved::Unknown;
        };
        match self.current.iter().find(|(_, s, _)| *s == storm.id) {
            Some((id, ..)) => Resolved::Current(id.clone()),
            None => Resolved::Gone(storm.id),
        }
    }

    /// A current cell's trend, as its storm's: the sample each of the storm's observations
    /// recorded under the SCIT ID it had then, so a storm keeps its trend through an ID change and
    /// a recycled ID does not splice another storm's samples into it. Samples from before the
    /// history began (an earlier scan's backfill) are taken from its current ID, as before, since
    /// nothing says otherwise; a cell with no storm gets its ID's samples.
    pub(crate) fn trend(
        &self,
        cell_id: &str,
        samples: &std::collections::HashMap<String, Vec<crate::ui::cell_window::CellSample>>,
    ) -> Vec<crate::ui::cell_window::CellSample> {
        let own = samples.get(cell_id).map(Vec::as_slice).unwrap_or(&[]);
        let Some(storm) = self.storm_of(cell_id) else {
            return own.to_vec();
        };
        // Before the history began, nothing says which storm an ID was; after, the history does.
        let first = self
            .history
            .storms()
            .iter()
            .filter_map(|s| s.observations.first().map(|o| o.time))
            .min();
        let secs = |s: &crate::ui::cell_window::CellSample| s.time.map(|t| t.timestamp());
        let mut out: Vec<crate::ui::cell_window::CellSample> = own
            .iter()
            .filter(|s| secs(s).zip(first).is_some_and(|(t, f)| t < f))
            .cloned()
            .collect();
        for o in &storm.observations {
            let Some(p) = o.provider_id.as_deref() else {
                continue;
            };
            if let Some(s) = samples
                .get(p)
                .and_then(|v| v.iter().find(|s| secs(s) == Some(o.time)))
            {
                out.push(*s);
            }
        }
        out.sort_by_key(|s| s.time);
        out.dedup_by_key(|s| s.time);
        out
    }

    /// Why a known storm is not in the current table, for a reader: merged into another (and
    /// which cell that is now), no longer tracked, or missed this scan.
    pub(crate) fn gone_note(&self, storm: wxdata::storm_history::StormId) -> String {
        use wxdata::storm_history::Lineage;
        let Some(s) = self.history.storm(storm) else {
            return "no longer tracked".into();
        };
        let merged = s.lineage.iter().rev().find_map(|l| match l {
            Lineage::MergedInto(x) => Some(*x),
            _ => None,
        });
        match merged {
            Some(x) => match self.current.iter().find(|(_, id, _)| *id == x) {
                Some((cell, ..)) => format!("merged into storm #{} (now cell {cell})", x.0),
                None => format!("merged into storm #{}", x.0),
            },
            None if s.closed => "no longer tracked".into(),
            None => "missed this scan".into(),
        }
    }

    /// The time of the SCIT table last fed.
    pub(crate) fn fed_scan(&self) -> Option<i64> {
        self.fed
    }

    /// Carry SCIT cell IDs from the table at `scan` to the current one, by storm: a renamed
    /// storm's ID becomes its new one, a storm no longer in the table is dropped, and an ID the
    /// history has no record of stays only if the current table still has it (`live`).
    pub(crate) fn carry_ids(
        &self,
        ids: &[String],
        scan: Option<i64>,
        live: &[String],
    ) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for id in ids {
            let now = match self.resolve(id, scan) {
                Resolved::Current(c) => Some(c),
                Resolved::Gone(_) => None,
                Resolved::Unknown => live.contains(id).then(|| id.clone()),
            };
            if let Some(n) = now.filter(|n| !out.contains(n)) {
                out.push(n);
            }
        }
        out
    }

    /// A storm's lineage in both directions, as phrases: the storm it split from, storms it
    /// absorbed, storms that split off it.
    fn lineage_parts(&self, s: &wxdata::storm_history::Storm) -> Vec<String> {
        use wxdata::storm_history::Lineage;
        let mut parts: Vec<String> = s
            .lineage
            .iter()
            .filter_map(|l| match l {
                Lineage::SplitFrom(p) => Some(format!("split from #{}", p.0)),
                Lineage::MergedInto(_) => None,
            })
            .collect();
        // What other storms' lineage says about this one.
        let related = |want: fn(&Lineage) -> Option<wxdata::storm_history::StormId>| {
            self.history
                .storms()
                .iter()
                .filter(|o| o.lineage.iter().any(|l| want(l) == Some(s.id)))
                .map(|o| format!("#{}", o.id.0))
                .collect::<Vec<_>>()
        };
        let absorbed = related(|l| match l {
            Lineage::MergedInto(x) => Some(*x),
            _ => None,
        });
        if !absorbed.is_empty() {
            parts.push(format!("absorbed {}", absorbed.join(", ")));
        }
        let children = related(|l| match l {
            Lineage::SplitFrom(x) => Some(*x),
            _ => None,
        });
        if !children.is_empty() {
            parts.push(format!("split off {}", children.join(", ")));
        }
        parts
    }

    /// The Storms table's lineage tag for a cell's storm (M2.2): "split" when it split from a
    /// storm or one split off it, "merge" when it absorbed one, both when both; with the phrases
    /// behind it for the row's hover. `None` for a storm with no lineage.
    pub(crate) fn lineage_mark(&self, cell_id: &str) -> Option<(&'static str, String)> {
        let s = self.storm_of(cell_id)?;
        let parts = self.lineage_parts(s);
        let split = parts.iter().any(|p| p.starts_with("split"));
        let merged = parts.iter().any(|p| p.starts_with("absorbed"));
        let tag = match (split, merged) {
            (true, true) => "split+merge",
            (true, false) => "split",
            (false, true) => "merge",
            (false, false) => return None,
        };
        Some((tag, parts.join("; ")))
    }

    /// The storm a SCIT cell of the current table belongs to.
    pub(crate) fn storm_of(&self, cell_id: &str) -> Option<&wxdata::storm_history::Storm> {
        let id = self.current.iter().find(|(c, ..)| c == cell_id)?.1;
        self.history.storm(id)
    }

    /// One line on a cell's storm: its stable ID, how long and over how many scans it has been
    /// tracked, the SCIT IDs it has carried, its lineage and whether its last link is tentative.
    pub(crate) fn describe(&self, cell_id: &str) -> Option<String> {
        use wxdata::storm_history::Confidence;
        let s = self.storm_of(cell_id)?;
        let first = s.observations.first()?.time;
        let last = s.observations.last()?.time;
        let mut line = format!(
            "storm #{} · {} over {} scan{}",
            s.id.0,
            if last > first {
                format!("tracked {} min", (last - first) / 60)
            } else {
                "first seen this scan".to_string()
            },
            s.observations.len(),
            if s.observations.len() == 1 { "" } else { "s" }
        );
        let mut ids: Vec<&str> = Vec::new();
        for o in s.observations.iter().rev() {
            if let Some(p) = o.provider_id.as_deref() {
                if !ids.contains(&p) {
                    ids.push(p);
                }
            }
        }
        if ids.len() > 1 {
            line.push_str(&format!(
                " · SCIT ID {} (earlier {})",
                ids[0],
                ids[1..].join(", ")
            ));
        }
        for part in self.lineage_parts(s) {
            line.push_str(&format!(" · {part}"));
        }
        if s.associations
            .last()
            .is_some_and(|a| a.confidence == Confidence::Tentative)
        {
            line.push_str(" · latest link tentative");
        }
        Some(line)
    }
}

/// SCIT's motion as m/s east and north: `mvt_deg` is the bearing it moves toward.
fn scit_motion_ms(c: &wxdata::level3::Cell) -> Option<(f64, f64)> {
    let (deg, kt) = (c.mvt_deg?, c.mvt_kt?);
    let ms = kt as f64 * 0.514_444;
    let r = (deg as f64).to_radians();
    Some((ms * r.sin(), ms * r.cos()))
}

impl HookEchoApp {
    /// The selected storm, current: `cell_popup` is a copy taken at the click, so the newest
    /// SCIT update of the same storm replaces it, keeping its position and attributes live.
    /// `None` when nothing is selected; the click-time copy when the storm has left the product.
    ///
    /// "The same storm" is the storm history's (ROADMAP_PARITY M2.2): SCIT recycles cell IDs and
    /// changes them, so a storm that took a new ID stays selected, and a different storm that took
    /// its old ID does not inherit the selection. Only when the history has no record of the pick
    /// is the ID matched as is.
    pub(crate) fn selected_storm(&self) -> Option<Cell> {
        self.selected_storm_live().map(|(c, _)| c)
    }

    /// [`Self::selected_storm`], and whether it is in the current SCIT table (`false`: the
    /// click-time copy).
    pub(crate) fn selected_storm_live(&self) -> Option<(Cell, bool)> {
        let picked = self.cell_popup.as_ref()?;
        let cells = self.active_storm_cells();
        let by_id = |id: &str| cells.iter().find(|c| !id.is_empty() && c.id == id);
        let resolved = self
            .dock
            .storm_ids
            .resolve(&picked.id, picked.time.map(|t| t.timestamp()));
        let live = match resolved {
            Resolved::Current(id) => by_id(&id),
            Resolved::Gone(_) => None,
            Resolved::Unknown => by_id(&picked.id),
        };
        Some(match live {
            Some(c) => (c.clone(), true),
            None => (picked.clone(), false),
        })
    }

    /// Storm-follow camera: re-lock onto the tracked cell in the freshly-applied volume and recenter
    /// the active pane on it. Called from the `Cells` apply arm, after the storm history has been
    /// fed the new table.
    ///
    /// The history decides which cell is the followed storm now (ROADMAP_PARITY M2.2): through a
    /// SCIT renumbering it follows the storm, a different storm that took its old ID is not
    /// adopted, a storm missing from one scan is held where it was, and a storm the history has
    /// closed ends the follow. Only when the history
    /// has no record (another radar's table, no times) is the SCIT ID matched, and failing that
    /// the nearest cell to where its last motion puts it.
    pub(crate) fn update_follow(&mut self) {
        let Some((fsite, last, since)) = self.follow_cell.take() else {
            return;
        };
        // Active site changed out from under the follow (site switch) → stop silently.
        if self.cells_site.as_deref() != Some(fsite.as_str()) {
            return;
        }
        let resolved = self
            .dock
            .storm_ids
            .resolve(&last.id, last.time.map(|t| t.timestamp()));
        let by_id = |id: &str| {
            self.storm_cells
                .iter()
                .find(|c| !c.id.is_empty() && c.id == id)
                .cloned()
        };
        let found = match &resolved {
            Resolved::Current(id) => by_id(id),
            Resolved::Gone(_) => None,
            Resolved::Unknown => by_id(&last.id),
        };
        if let Some(c) = found {
            self.recenter_follow(&c);
            self.follow_cell = Some((fsite, c, Instant::now()));
            return;
        }
        if let Resolved::Gone(storm) = resolved {
            // Missed this scan but still tracked (within the history's gap limit): hold where it
            // was rather than jump to whichever cell is near.
            let open = self
                .dock
                .storm_ids
                .history
                .storm(storm)
                .is_some_and(|s| !s.closed);
            if open {
                self.follow_notice = Some((
                    format!("{} not in this scan — holding", last.id),
                    Instant::now(),
                ));
                self.follow_cell = Some((fsite, last, since));
            } else {
                self.follow_notice =
                    Some((format!("Lost {} — follow ended", last.id), Instant::now()));
            }
            return;
        }
        // Renumber/miss: predict where the cell drifted and adopt the nearest new cell within 15 km.
        let elapsed_h = since.elapsed().as_secs_f64() / 3600.0;
        let pred = match (last.mvt_deg, last.mvt_kt) {
            (Some(dir), Some(kt)) if kt > 0.0 => crate::geo::destination_point(
                [last.lon, last.lat],
                dir as f64,
                kt as f64 * 1.852 * elapsed_h,
            ),
            _ => [last.lon, last.lat],
        };
        if let Some(c) =
            crate::app::nearest_cell(&self.storm_cells, pred[0], pred[1], 15.0).cloned()
        {
            self.recenter_follow(&c);
            self.follow_cell = Some((fsite, c, Instant::now()));
        } else {
            self.follow_notice = Some((format!("Lost {} — follow ended", last.id), Instant::now()));
            // follow_cell already taken → stays None.
        }
    }

    /// [`StormIdentity::trend`] for a cell of the current table.
    pub(crate) fn storm_trend(&self, cell_id: &str) -> Vec<crate::ui::cell_window::CellSample> {
        self.dock.storm_ids.trend(cell_id, &self.cell_trends)
    }

    /// Record what the active radar's storms are linked to at the current SCIT scan, when an
    /// input changed since it was last recorded (a detector finishing, a warning issued).
    fn record_storm_evidence(&mut self) {
        let Some(scan) = self.dock.storm_ids.fed else {
            return;
        };
        let (volume, volume_time) = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| (v.name.clone(), Some(v.time.timestamp())))
            .unwrap_or_default();
        let has_rot = self.rot_shown_cache.peek(&volume).is_some();
        let has_tds = self.tds_shown_cache.peek(&volume).is_some();
        let fused = self
            .llsd_cache
            .as_ref()
            .is_some_and(|(key, ..)| key.1 == volume);
        let key = EvidenceKey {
            scan,
            volume: volume.clone(),
            detectors: [has_rot, has_tds, fused],
            features: features_hash(&[self.active_alert_features(), &self.probsevere]),
        };
        if self.dock.storm_ids.evidence_key.as_ref() == Some(&key) {
            return;
        }
        let circulations = if has_rot || has_tds {
            let rot = self.rot_shown_cache.peek(&volume).cloned();
            let tds = self.tds_shown_cache.peek(&volume).cloned();
            self.cached_circulations(&volume, &rot.unwrap_or_default(), &tds.unwrap_or_default())
        } else {
            Vec::new()
        };
        let cells = self.active_storm_cells().to_vec();
        let warnings = self.active_alert_features().to_vec();
        let probsevere = self.probsevere.clone();
        self.dock.storm_ids.record_evidence(
            key,
            &cells,
            &warnings,
            &probsevere,
            &circulations,
            volume_time,
        );
    }

    pub(super) fn dock_storms(&mut self, host: Host<'_>) {
        // The storm history follows every SCIT table, open or not, so identities survive the
        // window being closed.
        let site = self.views[self.active].site.clone();
        let cells_time = self.active_storm_cells().iter().find_map(|c| c.time);
        if cells_time.is_some_and(|t| self.dock.storm_ids.fed != Some(t.timestamp()))
            || self.dock.storm_ids.site != site
        {
            let cells = self.active_storm_cells().to_vec();
            self.dock.storm_ids.feed(site.as_deref(), &cells);
        }
        self.record_storm_evidence();
        if !self.dock.storms.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.storms.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.storms.collapsed;
        let cells: Vec<wxdata::level3::Cell> = self.active_storm_cells().to_vec();
        let archive = self.archive_bucket().is_some();
        let couplets: &[wxdata::rotation::CoupletHit] = match &self.couplet_cache {
            Some((_, (hits, ..))) => hits,
            None => &[],
        };
        let explanations =
            wxdata::cellscore::score_all_explained(&cells, &self.probsevere, couplets);
        let scores: Vec<u8> = explanations.iter().map(|e| e.score).collect();
        // Each row's stable storm, beside SCIT's own (recycled) ID.
        let stable: Vec<Option<u64>> = cells
            .iter()
            .map(|c| self.dock.storm_ids.storm_of(&c.id).map(|s| s.id.0))
            .collect();
        let lineage: Vec<Option<(&'static str, String)>> = cells
            .iter()
            .map(|c| self.dock.storm_ids.lineage_mark(&c.id))
            .collect();
        let (sort, desc) = (self.dock.storm_sort, self.dock.storm_desc);
        let mut query = self.dock.storm_query.clone();
        // The row of the selected storm, when it is in this table: not whichever cell has the ID
        // it was picked by, which SCIT may have given to another storm since.
        let selected = self
            .selected_storm_live()
            .filter(|(_, live)| *live)
            .map(|(c, _)| c.id);
        let title = if cells.is_empty() {
            "Storms".to_string()
        } else {
            format!("Storms ({})", cells.len())
        };
        let list_h = (map_rect.height() - 170.0).clamp(90.0, 560.0);
        let mut header = ws::HeaderAction::None;
        let mut pick: Option<(usize, StormAction)> = None;
        let mut resort = None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_storms",
                place,
                width: STORMS_W,
                float_at: map_rect.left_top() + egui::vec2(12.0 + LEFT_WIDTH + 12.0, 60.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::TORNADO,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                if cells.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(ws::text(
                            if archive {
                                "Storm cells are live only: SCIT is not archived."
                            } else {
                                "No storm cells from this radar right now."
                            },
                            12.0,
                            t.text_dim,
                        ));
                    });
                    ui.add_space(8.0);
                    return;
                }
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("Find cell ID…")
                            .desired_width((ui.available_width() - 52.0).max(80.0)),
                    );
                    if ui
                        .add_enabled(!query.is_empty(), egui::Button::new("Clear"))
                        .named("Clear the storm cell filter")
                        .clicked()
                    {
                        query.clear();
                    }
                });
                let order =
                    matching_order(&cells, sorted_indices(&cells, &scores, sort, desc), &query);
                ui.label(ws::text(
                    format!("{} of {} cells", order.len(), cells.len()),
                    11.0,
                    t.text_dim,
                ));
                // Keep columns readable even when the operator narrows a side dock to 240 pt.
                // Horizontal scrolling carries the headers and rows together.
                let table_h = (ui.available_height() - 48.0).max(ROW_H * 2.0);
                let table_h = if floating {
                    table_h.min(list_h)
                } else {
                    table_h
                };
                egui::ScrollArea::horizontal()
                    .id_salt("dock_storms_columns")
                    .auto_shrink([false, floating])
                    .max_height(table_h)
                    .show(ui, |ui| {
                        ui.set_min_width(COLS.iter().map(|col| col.2).sum::<f32>() + 8.0 + 32.0);
                        // Header row: each column sorts.
                        let (hr, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_H),
                            Sense::hover(),
                        );
                        ui.painter().rect_filled(hr, 0.0, t.panel_hi);
                        let mut x = hr.left() + 8.0;
                        for (i, (label, key, w)) in COLS.iter().enumerate() {
                            let r =
                                Rect::from_min_size(egui::pos2(x, hr.top()), egui::vec2(*w, ROW_H));
                            x += w;
                            let resp = ui
                                .interact(r, ui.id().with(("storm_col", i)), Sense::click())
                                .on_hover_text(match key {
                                    SortCol::Rank => "Severity score, 0-100",
                                    SortCol::Range => "Range from the radar, NM",
                                    SortCol::Top => "Cell top, kft",
                                    SortCol::Vil => "Water aloft, kg/m\u{b2}",
                                    SortCol::Posh => "Probability of severe hail, % (POSH)",
                                    SortCol::Hail => "Max expected hail size, in",
                                    SortCol::Id => "Cell identifier",
                                    SortCol::MaxDbz => "Maximum reflectivity, dBZ",
                                    SortCol::Poh => "Probability of hail, %",
                                })
                                .named_toggle(&format!("Sort by {label}"), *key == sort);
                            let on = *key == sort;
                            let text = if on {
                                format!("{label}{}", if desc { "\u{25be}" } else { "\u{25b4}" })
                            } else {
                                label.to_string()
                            };
                            ui.painter().text(
                                r.left_center(),
                                egui::Align2::LEFT_CENTER,
                                text,
                                FontId::proportional(11.0),
                                if on { t.accent } else { t.text_dim },
                            );
                            if resp.clicked() {
                                resort = Some(*key);
                            }
                        }
                        ui.painter().text(
                            egui::pos2(hr.right() - 8.0, hr.center().y),
                            egui::Align2::RIGHT_CENTER,
                            "Rot",
                            FontId::proportional(11.0),
                            t.text_dim,
                        );
                        if order.is_empty() {
                            ui.label(ws::text(
                                "No cells match this ID. Clear the filter to see all cells.",
                                12.0,
                                t.text_dim,
                            ));
                        }
                        let scroll = egui::ScrollArea::vertical()
                            .id_salt("dock_storms_rows")
                            .auto_shrink([false, floating])
                            .max_height((table_h - ROW_H - 14.0).max(ROW_H));
                        scroll.show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (n, &i) in order.iter().enumerate() {
                                let c = &cells[i];
                                let (mut cols, flags) = row_cells(c, scores.get(i).copied());
                                if let Some(Some(id)) = stable.get(i) {
                                    cols[1] = format!("{} #{id}", cols[1]);
                                }
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), ROW_H),
                                    Sense::hover(),
                                );
                                let row_id = ui.id().with(("storm_row", &c.id));
                                let focused = ui.memory(|memory| memory.has_focus(row_id));
                                // Consume Enter before interact turns it into a generic click.
                                let details = focused
                                    && ui.input_mut(|input| {
                                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                    });
                                let resp = ui.interact(r, row_id, Sense::click());
                                if resp.has_focus() {
                                    crate::hotkeys::reserve_navigation(ui.ctx(), row_id);
                                }
                                ui.memory_mut(|memory| {
                                    memory.set_focus_lock_filter(
                                        row_id,
                                        egui::EventFilter {
                                            vertical_arrows: true,
                                            ..Default::default()
                                        },
                                    )
                                });
                                let on = selected.as_deref() == Some(c.id.as_str());
                                let p = ui.painter();
                                if on || resp.has_focus() {
                                    p.rect_filled(r, 0.0, t.accent_soft().gamma_multiply(0.6));
                                    p.rect_filled(
                                        Rect::from_min_size(r.min, egui::vec2(2.0, r.height())),
                                        0.0,
                                        t.accent,
                                    );
                                } else if resp.hovered() {
                                    p.rect_filled(r, 0.0, t.panel_hi);
                                } else if n % 2 == 1 {
                                    p.rect_filled(r, 0.0, t.panel_hi.gamma_multiply(0.45));
                                }
                                let mut x = r.left() + 8.0;
                                for (k, text) in cols.iter().enumerate() {
                                    // Severity and the hail columns take the warning colour when high,
                                    // alongside the number itself.
                                    let hot = match k {
                                        0 => scores.get(i).is_some_and(|s| *s >= 60),
                                        6 => c.posh.is_some_and(|p| p >= 50),
                                        7 => c.hail_in.is_some_and(|h| h >= 1.0),
                                        _ => false,
                                    };
                                    p.with_clip_rect(
                                        Rect::from_min_size(
                                            egui::pos2(x, r.top()),
                                            egui::vec2(COLS[k].2 - 2.0, ROW_H),
                                        )
                                        .intersect(ui.clip_rect()),
                                    )
                                    .text(
                                        egui::pos2(x, r.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        text,
                                        FontId::monospace(11.0),
                                        if hot {
                                            t.warn
                                        } else if k == 1 {
                                            egui::Color32::WHITE
                                        } else {
                                            t.text
                                        },
                                    );
                                    x += COLS[k].2;
                                }
                                paint_row_marks(
                                    p,
                                    r,
                                    &flags,
                                    lineage[i].as_ref().map(|(tag, _)| *tag),
                                    &t,
                                );
                                let resp = resp.named_toggle(
                                    &format!(
                                        "Storm {}: severity {}{}{}",
                                        c.id,
                                        cols[0],
                                        if flags.is_empty() {
                                            String::new()
                                        } else {
                                            format!(", rotation {flags}")
                                        },
                                        lineage[i]
                                            .as_ref()
                                            .map_or_else(String::new, |(_, why)| format!(
                                                ", {why}"
                                            ))
                                    ),
                                    on,
                                );
                                resp.clone().on_hover_ui(|ui| {
                                    ui.label(format!("SCIT cell {}", c.id));
                                    if let Some(time) = c.time {
                                        ui.label(format!(
                                            "Source time: {} UTC",
                                            time.format("%Y-%m-%d %H:%M:%S")
                                        ));
                                    } else {
                                        ui.weak("Source time unavailable");
                                    }
                                    for line in explanations[i].lines() {
                                        ui.label(line);
                                    }
                                    if let Some((_, why)) = &lineage[i] {
                                        ui.label(format!("Lineage: {why}"));
                                    }
                                    ui.weak("Right-click for details, centering or manual motion.");
                                });
                                resp.context_menu(|ui| {
                                    if ui.button("Details…").clicked() {
                                        pick = Some((i, StormAction::Details));
                                        ui.close();
                                    }
                                    if ui.button("Center on map").clicked() {
                                        pick = Some((i, StormAction::Center));
                                        ui.close();
                                    }
                                    let has_motion = c.mvt_deg.is_some() && c.mvt_kt.is_some();
                                    if ui
                                        .add_enabled(
                                            has_motion,
                                            egui::Button::new("Track manually"),
                                        )
                                        .on_disabled_hover_text(
                                            "SCIT has not reported motion for this cell.",
                                        )
                                        .clicked()
                                    {
                                        pick = Some((i, StormAction::Track));
                                        ui.close();
                                    }
                                });
                                if resp.double_clicked() {
                                    pick = Some((i, StormAction::Center));
                                } else if resp.clicked() {
                                    resp.request_focus();
                                    crate::hotkeys::reserve_navigation(ui.ctx(), row_id);
                                    pick = Some((i, StormAction::Select));
                                }
                                if focused {
                                    let step = ui.input_mut(|input| {
                                        if input.consume_key(
                                            egui::Modifiers::NONE,
                                            egui::Key::ArrowDown,
                                        ) {
                                            Some(1)
                                        } else if input
                                            .consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                                        {
                                            Some(-1)
                                        } else {
                                            None
                                        }
                                    });
                                    if let Some(step) = step {
                                        let at = (n as isize + step)
                                            .clamp(0, order.len() as isize - 1)
                                            as usize;
                                        let next = order[at];
                                        ui.memory_mut(|memory| {
                                            memory.request_focus(
                                                ui.id().with(("storm_row", &cells[next].id)),
                                            )
                                        });
                                        crate::hotkeys::reserve_navigation(
                                            ui.ctx(),
                                            ui.id().with(("storm_row", &cells[next].id)),
                                        );
                                        // The previous row was already drawn when stepping up;
                                        // scroll its position now rather than waiting for focus.
                                        ui.scroll_to_rect(
                                            r.translate(egui::vec2(
                                                0.0,
                                                (at as f32 - n as f32) * ROW_H,
                                            )),
                                            Some(egui::Align::Center),
                                        );
                                        pick = Some((next, StormAction::Select));
                                    }
                                    if details {
                                        pick = Some((i, StormAction::Details));
                                    }
                                }
                                if resp.gained_focus() {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                            }
                        });
                    });
                // A line under the table saying what the flags mean.
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter()
                    .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.line));
                ui.label(ws::text(
                    "T tornado vortex · M mesocyclone · split/merge storm lineage",
                    10.5,
                    t.text_faint,
                ));
                ui.label(ws::text(
                    "↑/↓ select · Enter details · double-click centers",
                    10.5,
                    t.text_faint,
                ));
            },
        );
        self.dock.storm_query = query;
        self.dock.apply_header(DockWin::Storms, header);
        if let Some(key) = resort {
            if key == self.dock.storm_sort {
                self.dock.storm_desc = !self.dock.storm_desc;
            } else {
                self.dock.storm_sort = key;
                // Big numbers first, except names and range, which read best from the top down.
                self.dock.storm_desc = !matches!(key, SortCol::Id | SortCol::Range);
            }
        }
        if let Some((i, action)) = pick {
            let c = cells[i].clone();
            if matches!(action, StormAction::Center) {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
                cam.zoom = cam.zoom.max(8.0);
            }
            if matches!(action, StormAction::Track) {
                self.track_cell_manually(&c);
            }
            if matches!(action, StormAction::Details) {
                self.cell_details = true;
                self.dock.bring_forward(DockWin::Cell);
            }
            self.select_storm_from(c, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtering_preserves_severity_order_and_source_indices() {
        let cells: Vec<_> = ["A1", "B2", "A3"]
            .into_iter()
            .map(|id| wxdata::level3::Cell {
                id: id.into(),
                ..Default::default()
            })
            .collect();
        let order = sorted_indices(&cells, &[20, 90, 70], SortCol::Rank, true);
        assert_eq!(matching_order(&cells, order.clone(), " a "), vec![2, 0]);
        assert_eq!(matching_order(&cells, order.clone(), ""), order);
        assert!(matching_order(&cells, order, "missing").is_empty());
    }

    #[test]
    fn scit_motion_and_a_storm_history_line_read_as_a_person_would() {
        use wxdata::level3::Cell;
        let at = |id: &str, lon: f64, min: i64| Cell {
            id: id.into(),
            lon,
            lat: 35.3,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            mvt_deg: Some(90.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let east = super::scit_motion_ms(&at("O7", -97.5, 0)).unwrap();
        assert!(
            (east.0 - 15.43).abs() < 0.01 && east.1.abs() < 1e-6,
            "{east:?}"
        );
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("O7", -97.5, 0)]);
        ids.feed(Some("KTLX"), &[at("O7", -97.45, 5)]);
        ids.feed(Some("KTLX"), &[at("K3", -97.40, 10)]);
        let line = ids.describe("K3").expect("tracked");
        assert!(
            line.starts_with("storm #1 · tracked 10 min over 3 scans"),
            "{line}"
        );
        assert!(line.contains("SCIT ID K3 (earlier O7)"), "{line}");
        // The same table again feeds nothing; another radar starts over.
        ids.feed(Some("KTLX"), &[at("K3", -97.40, 10)]);
        assert_eq!(ids.storm_of("K3").unwrap().observations.len(), 3);
        ids.feed(Some("KOUN"), &[at("A1", -97.4, 15)]);
        assert!(ids.storm_of("K3").is_none());
    }

    #[test]
    fn a_storm_keeps_what_it_was_linked_to_across_scans_and_a_seek_back() {
        use wxdata::level3::Cell;
        use wxdata::overlay::{AlertInfo, FeatureKind, GeoFeature};
        let at = |id: &str, lon: f64, min: i64, posh: Option<i32>| Cell {
            id: id.into(),
            lon,
            lat: 35.3,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            posh,
            ..Default::default()
        };
        let box_ = |lon: f64| {
            vec![vec![
                [lon - 0.2, 35.1],
                [lon + 0.2, 35.1],
                [lon + 0.2, 35.5],
                [lon - 0.2, 35.5],
            ]]
        };
        let warning = GeoFeature {
            rings: box_(-97.45),
            fill: [0; 4],
            stroke: [0; 4],
            kind: FeatureKind::Warning,
            title: "Tornado Warning".into(),
            detail: String::new(),
            alert: Some(AlertInfo {
                id: "urn:1".into(),
                event: "Tornado Warning".into(),
                headline: String::new(),
                area: String::new(),
                description: String::new(),
                instruction: String::new(),
                expires: None,
                issued: None,
                effective: None,
                max_hail_in: None,
                max_wind: None,
                tornado_detection: None,
                damage_threat: None,
                source: None,
                motion: None,
                vtec: None,
            }),
        };
        let watch = GeoFeature {
            kind: FeatureKind::Watch,
            title: "Tornado Watch".into(),
            alert: None,
            ..warning.clone()
        };
        let ps = |p: &str| GeoFeature {
            rings: box_(-97.45),
            kind: FeatureKind::ProbSevere,
            title: p.into(),
            detail: "ProbSevere storm 4321\nSevere: 80%".into(),
            alert: None,
            ..warning.clone()
        };
        let key = |scan: i64, features: u64| EvidenceKey {
            scan,
            volume: String::new(),
            detectors: [false; 3],
            features,
        };
        let mut ids = super::StormIdentity::default();
        let scans = [
            (vec![at("O7", -97.5, 0, Some(30))], "Tor 40%"),
            (vec![at("O7", -97.45, 5, Some(50))], "Tor 62%"),
            (vec![at("K3", -97.40, 10, Some(50))], "Tor 62%"),
        ];
        let fmt = |s: i64| format!("+{}", (s - 1_700_000_000) / 60);
        let feed = |ids: &mut super::StormIdentity, cells: &Vec<Cell>, p: &str| {
            ids.feed(Some("KTLX"), cells);
            let scan = ids.fed.unwrap();
            let features = [warning.clone(), watch.clone()];
            ids.record_evidence(key(scan, 1), cells, &features, &[ps(p)], &[], None);
        };
        for (cells, p) in &scans {
            feed(&mut ids, cells, p);
        }
        let lines = ids.evidence_lines("K3", fmt);
        assert_eq!(lines.len(), 3, "{lines:#?}");
        assert!(
            lines
                .iter()
                .any(|l| l
                    == "ProbSevere object 4321 — Tor 40% → Tor 62% (+0–+10, 3 scans; covers it)"),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("warning urn:1 — Tornado Warning (+0–+10")),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("cell O7: POSH 30% → cell O7: POSH 50% → cell K3: POSH 50%")),
            "the SCIT ID change stays in the one hail track: {lines:#?}"
        );
        assert!(lines.iter().all(|l| !l.contains("Watch")), "{lines:#?}");
        // The same inputs again record nothing new; a seek back replays to the same lines.
        let before = ids.evidence_lines("K3", fmt);
        let scan = ids.fed.unwrap();
        ids.record_evidence(key(scan, 1), &scans[2].0, &[], &[], &[], None);
        assert_eq!(ids.evidence_lines("K3", fmt), before);
        for (cells, p) in &scans {
            feed(&mut ids, cells, p);
        }
        assert_eq!(ids.evidence_lines("K3", fmt), before);
    }

    #[test]
    fn a_pick_follows_its_storm_not_the_scit_id_it_was_picked_by() {
        use wxdata::level3::Cell;
        let at = |id: &str, lon: f64, min: i64| Cell {
            id: id.into(),
            lon,
            lat: 35.3,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            ..Default::default()
        };
        let t0 = Some(1_700_000_000);
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("O7", -97.5, 0)]);
        assert_eq!(ids.resolve("O7", t0), Resolved::Current("O7".into()));
        // SCIT renames the storm and gives its old ID to a new storm 150 km east.
        ids.feed(Some("KTLX"), &[at("K3", -97.47, 5), at("O7", -95.8, 5)]);
        assert_eq!(
            ids.resolve("O7", t0),
            Resolved::Current("K3".into()),
            "the pick stays on its storm, not on the cell that now has its ID"
        );
        // The storm drops out; the recycled O7 is still there and still not it.
        ids.feed(Some("KTLX"), &[at("O7", -95.75, 10)]);
        let Resolved::Gone(storm) = ids.resolve("O7", t0) else {
            panic!("{:?}", ids.resolve("O7", t0));
        };
        assert!(
            !ids.history.storm(storm).unwrap().closed,
            "one missed scan holds a follow rather than ending it"
        );
        // A pick the history never saw (another time, another radar) is unknown.
        assert_eq!(ids.resolve("O7", Some(1)), Resolved::Unknown);
        assert_eq!(ids.resolve("O7", None), Resolved::Unknown);
    }

    #[test]
    fn a_trend_is_its_storms_through_a_rename_and_never_a_recycled_ids() {
        use crate::ui::cell_window::CellSample;
        use wxdata::level3::Cell;
        let t = |min: i64| chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0);
        let at = |id: &str, lon: f64, min: i64| Cell {
            id: id.into(),
            lon,
            lat: 35.3,
            time: t(min),
            ..Default::default()
        };
        let sample = |min: i64, dbz: f32| CellSample {
            vil: None,
            top: None,
            dbz: Some(dbz),
            severity: None,
            time: t(min),
            dbz_hgt: None,
        };
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("O7", -97.5, 0)]);
        ids.feed(Some("KTLX"), &[at("O7", -97.47, 5)]);
        ids.feed(Some("KTLX"), &[at("K3", -97.44, 10), at("O7", -95.8, 10)]);
        // The samples as they are kept, by SCIT ID: O7 holds both storms', the backfill included.
        let samples: std::collections::HashMap<String, Vec<CellSample>> = [
            (
                "O7".to_string(),
                vec![sample(0, 50.0), sample(5, 55.0), sample(10, 20.0)],
            ),
            ("K3".to_string(), vec![sample(-5, 44.0), sample(10, 60.0)]),
        ]
        .into();
        let dbz = |v: Vec<CellSample>| v.iter().map(|s| s.dbz.unwrap()).collect::<Vec<_>>();
        assert_eq!(
            dbz(ids.trend("K3", &samples)),
            [44.0, 50.0, 55.0, 60.0],
            "its backfill, its samples as O7, then as K3"
        );
        assert_eq!(
            dbz(ids.trend("O7", &samples)),
            [20.0],
            "the new O7 is its own storm, with none of the old O7's samples"
        );
    }

    #[test]
    fn a_split_and_merge_read_on_both_storms_and_a_merged_pick_says_where_it_went() {
        use wxdata::level3::Cell;
        let at = |id: &str, lat: f64, min: i64| Cell {
            id: id.into(),
            lon: -97.5,
            lat,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            ..Default::default()
        };
        let t1 = Some(1_700_000_000 + 5 * 60);
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("A1", 35.3, 0)]);
        // It splits: a second cell 3 km south.
        ids.feed(Some("KTLX"), &[at("A1", 35.3045, 5), at("B2", 35.273, 5)]);
        let parent = ids.describe("A1").unwrap();
        assert!(parent.contains("split off #2"), "{parent}");
        assert!(ids.describe("B2").unwrap().contains("split from #1"));
        // They merge back into one cell between them.
        ids.feed(Some("KTLX"), &[at("C3", 35.3, 10)]);
        let survivor = ids.describe("C3").unwrap();
        let (kept, gone) = match (ids.resolve("A1", t1), ids.resolve("B2", t1)) {
            (Resolved::Current(c), Resolved::Gone(g))
            | (Resolved::Gone(g), Resolved::Current(c)) => (c, g),
            other => panic!("{other:?}"),
        };
        assert_eq!(kept, "C3");
        assert!(
            survivor.contains(&format!("absorbed #{}", gone.0)),
            "{survivor}"
        );
        let note = ids.gone_note(gone);
        assert!(
            note.starts_with("merged into storm #") && note.ends_with("(now cell C3)"),
            "{note}"
        );
        // The table row's tag: the survivor split earlier (one of the pair split from the
        // other) and has now absorbed its partner.
        let (tag, why) = ids.lineage_mark("C3").unwrap();
        assert!(tag == "merge" || tag == "split+merge", "{tag}");
        assert!(why.contains(&format!("absorbed #{}", gone.0)), "{why}");
    }

    /// Rows with each lineage tag beside and without rotation flags, drawn by the table's own
    /// row painters, for review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the storm row lineage capture"]
    fn gpu_storm_row_lineage_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the rows");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m2.2");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let rows = [
            ("K3 #12", "TM", Some("split+merge")),
            ("Q4 #7", "", Some("split")),
            ("A1 #3", "M", Some("merge")),
            ("B9 #15", "T", None),
        ];
        gpu.save(&destination.join("storm-row-lineage.png"), 300, 120, |ui| {
            ws::panel_frame(&t).show(ui, |ui| {
                for (n, (id, flags, tag)) in rows.iter().enumerate() {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(280.0, 22.0), Sense::hover());
                    if n % 2 == 1 {
                        ui.painter()
                            .rect_filled(r, 0.0, t.panel_hi.gamma_multiply(0.45));
                    }
                    ui.painter().text(
                        egui::pos2(r.left() + 8.0, r.center().y),
                        egui::Align2::LEFT_CENTER,
                        *id,
                        FontId::monospace(11.0),
                        egui::Color32::WHITE,
                    );
                    paint_row_marks(ui.painter(), r, flags, *tag, &t);
                }
            });
        })
        .unwrap();
    }

    #[test]
    fn a_storm_without_lineage_has_no_tag_and_a_split_tags_both() {
        use wxdata::level3::Cell;
        let at = |id: &str, lat: f64, min: i64| Cell {
            id: id.into(),
            lon: -97.5,
            lat,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            ..Default::default()
        };
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("A1", 35.3, 0)]);
        assert_eq!(ids.lineage_mark("A1"), None);
        ids.feed(Some("KTLX"), &[at("A1", 35.3045, 5), at("B2", 35.273, 5)]);
        assert_eq!(
            ids.lineage_mark("A1"),
            Some(("split", "split off #2".to_string()))
        );
        assert_eq!(
            ids.lineage_mark("B2"),
            Some(("split", "split from #1".to_string()))
        );
        assert_eq!(ids.lineage_mark("Z9"), None, "a cell not in the table");
    }

    #[test]
    fn an_open_set_of_storms_carries_through_a_rename_and_drops_a_recycled_id() {
        use wxdata::level3::Cell;
        let at = |id: &str, lon: f64, min: i64| Cell {
            id: id.into(),
            lon,
            lat: 35.3,
            time: chrono::DateTime::from_timestamp(1_700_000_000 + min * 60, 0),
            ..Default::default()
        };
        let mut ids = super::StormIdentity::default();
        ids.feed(Some("KTLX"), &[at("O7", -97.5, 0), at("Q2", -96.5, 0)]);
        let then = ids.fed_scan();
        // O7 is renamed K3 and its old ID goes to a new storm far away; Q2 vanishes.
        ids.feed(Some("KTLX"), &[at("K3", -97.47, 5), at("O7", -95.0, 5)]);
        let live: Vec<String> = ["K3", "O7"].map(String::from).to_vec();
        let open = ["O7", "Q2"].map(String::from);
        assert_eq!(ids.carry_ids(&open, then, &live), ["K3"]);
        // Without a record (another radar's history), only what is still there stays.
        let fresh = super::StormIdentity::default();
        assert_eq!(fresh.carry_ids(&open, then, &live), ["O7"]);
    }

    #[test]
    fn a_row_says_unknown_plainly_and_flags_rotation_in_letters() {
        let c = wxdata::level3::Cell {
            id: "Q4".into(),
            range_nm: Some(41.2),
            max_dbz: Some(63.0),
            posh: Some(70),
            hail_in: Some(1.75),
            meso: Some("M".into()),
            ..Default::default()
        };
        let (cols, flags) = row_cells(&c, Some(82));
        assert_eq!(cols[0], "82");
        assert_eq!(cols[1], "Q4");
        assert_eq!(cols[2], "41");
        assert_eq!(cols[4], "\u{2014}", "no top reported");
        assert_eq!(cols[6], "70%");
        assert_eq!(cols[7], "1.75");
        assert_eq!(flags, "M");
        let widths: f32 = COLS.iter().map(|c| c.2).sum();
        assert!(
            widths + 8.0 + 24.0 <= STORMS_W,
            "the columns and flags fit the dock"
        );
    }
}
