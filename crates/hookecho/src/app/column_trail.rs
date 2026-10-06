//! A trail of the active pane's column user product (ROADMAP_PARITY M3.4): the per-cell extremum
//! of that product over the loop's volumes in the window ending at the playhead, drawn as
//! `FieldLayer::UserColumnTrail` with the same window and kept end as the radar trail.
//!
//! Each volume's product is evaluated off the UI thread from the decode cache, a couple of volumes
//! per job, and kept by scan time in a [`wxdata::extrema::GridTrail`]; moving the playhead
//! re-reads the exact window. The product's identity (definition, palette, the environmental
//! heights it reads and their source) keys the trail, so an edit starts it over. A formula that
//! reads an isotherm uses only volumes whose environmental epoch is the reading's own — the live
//! analysis while following, the same synoptic sounding when scrubbing — and the rest are left out
//! and counted, never evaluated against another time's environment.
use super::*;
use wxdata::extrema::{Extremum, GridMerge, GridTrail, GridWindowTrail};

/// Volumes evaluated per worker job; jobs run one at a time.
const FRAMES_PER_JOB: usize = 2;
const MAX_FRAMES: usize = if cfg!(target_os = "android") { 32 } else { 64 };

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnTrailKey {
    site: Option<String>,
    product: String,
    identity: u64,
    keep: Extremum,
    window_min: u16,
}

pub(crate) struct ColumnTrailState {
    key: ColumnTrailKey,
    trail: GridTrail,
    /// Volumes left out (another environment epoch, failed evaluation, another grid), by name.
    skipped: std::collections::HashMap<String, String>,
    /// The job in flight, by number; a delivery for another number is dropped.
    inflight: Option<u64>,
    next_job: u64,
    anchor: Option<i64>,
    pub(crate) shown: Option<GridWindowTrail>,
    pub(crate) name: String,
    pub(crate) units: String,
    /// The product's own colour table (a moment's), when it names one.
    spec_table: Option<crate::colormap::ColorTable>,
    /// The product's own fixed range (or its table's span), when it has one.
    spec_range: Option<(f32, f32)>,
    /// What the shown grid is coloured with and over.
    pub(crate) table: crate::colormap::ColorTable,
    pub(crate) range: Option<(f32, f32)>,
    /// Volumes in the window still to evaluate, for the legend.
    pub(crate) pending: usize,
}

pub(crate) struct ColumnTrailDelivery {
    key: ColumnTrailKey,
    job: u64,
    frames: Vec<(String, i64, Result<wxdata::mrms::MrmsField, String>)>,
}

impl HookEchoApp {
    fn column_trail_key(&self, idx: usize, spec: &column_product::ColumnSpec) -> ColumnTrailKey {
        ColumnTrailKey {
            site: self.views[idx].site.clone(),
            product: spec.name.clone(),
            identity: column_product::product_identity(&self.settings, self.palettes.gen, spec),
            keep: if self.filters.trail_keep_min {
                Extremum::Min
            } else {
                Extremum::Max
            },
            window_min: self.filters.trail_window_min,
        }
    }

    /// The playhead time (seconds) of pane `idx` and the cached volumes in the window ending at
    /// it, oldest first.
    fn column_trail_window(&self, idx: usize, window_s: i64) -> Option<(i64, Vec<(String, i64)>)> {
        let tl = &self.views[idx].timeline;
        let upto = &tl.frames[..(tl.playhead + 1).min(tl.frames.len())];
        let anchor = upto.iter().rev().find_map(|id| id.date_time())?.timestamp();
        let wanted = upto
            .iter()
            .filter_map(|id| Some((id.name().to_string(), id.date_time()?.timestamp())))
            .filter(|(name, t)| {
                *t <= anchor && *t >= anchor - window_s && self.scan_cache.contains(name)
            })
            .collect();
        Some((anchor, wanted))
    }

    /// Advance the trail for the active pane: retain the window at its playhead, start a job for
    /// volumes not yet evaluated, and stage the shown grid when it changed.
    pub(crate) fn advance_column_trail(&mut self, ctx: &egui::Context) {
        let idx = self.active;
        let layer = crate::render::FieldLayer::UserColumnTrail;
        if !self.views[idx].fields_on.contains(&layer) {
            self.column_trail = None;
            return;
        }
        let Some(Ok(spec)) = self.column_spec(idx) else {
            self.column_trail = None;
            return;
        };
        if !spec.env.missing_for(&spec.expr).is_empty() {
            // The single-volume product asks for the reading; nothing to evaluate until it lands.
            self.column_trail = None;
            return;
        }
        let window_s = i64::from(self.filters.trail_window_min) * 60;
        let Some((anchor, wanted)) = self.column_trail_window(idx, window_s) else {
            self.column_trail = None;
            return;
        };
        let key = self.column_trail_key(idx, &spec);
        if self.column_trail.as_ref().is_none_or(|s| s.key != key) {
            self.column_trail = Some(ColumnTrailState {
                trail: GridTrail::new(key.keep, window_s, MAX_FRAMES),
                key: key.clone(),
                skipped: Default::default(),
                inflight: None,
                next_job: 0,
                anchor: None,
                shown: None,
                name: spec.name.clone(),
                units: spec.units.clone(),
                spec_table: spec.table.clone(),
                spec_range: spec.range,
                table: crate::colormap::ramp_table(0.0, 1.0),
                range: None,
                pending: 0,
            });
        }
        // Environment epochs: a formula reading an isotherm uses only volumes of the reading's
        // own epoch.
        let reads_env = spec.expr.inputs().iter().any(|i| {
            matches!(
                i,
                wxdata::udp::Input::FreezingLevelM
                    | wxdata::udp::Input::Minus10cHeightM
                    | wxdata::udp::Input::Minus20cHeightM
            )
        });
        let reading_epoch = self.env_levels_for(idx).map(|l| l.epoch);
        let following = self.views[idx].timeline.following;
        let Some(state) = self.column_trail.as_mut() else {
            return;
        };
        let changed = state.anchor != Some(anchor);
        if changed {
            state.trail.retain_window(anchor);
            state.anchor = Some(anchor);
        }
        if reads_env {
            for (name, t) in &wanted {
                let epoch = (!following).then(|| {
                    wxdata::raob::synoptic_before(
                        chrono::DateTime::from_timestamp(*t, 0).unwrap_or_default(),
                    )
                });
                if reading_epoch != Some(epoch) {
                    state
                        .skipped
                        .entry(name.clone())
                        .or_insert_with(|| "its environment is another time's".into());
                }
            }
        }
        let todo: Vec<(String, i64)> = wanted
            .iter()
            .filter(|(name, t)| {
                !state.trail.times().any(|h| h == *t) && !state.skipped.contains_key(name)
            })
            .cloned()
            .collect();
        state.pending = todo.len();
        if state.inflight.is_none() && !todo.is_empty() {
            let job = state.next_job;
            state.next_job += 1;
            state.inflight = Some(job);
            let frames: Vec<(String, i64, Option<Arc<Scan>>)> = todo
                .into_iter()
                .take(FRAMES_PER_JOB)
                .map(|(name, t)| {
                    let scan = self.scan_cache.peek(&name).map(Arc::clone);
                    (name, t, scan)
                })
                .collect();
            let tx = self.overlay_tx.clone();
            let ctx = ctx.clone();
            let key = key.clone();
            let (expr, env) = (spec.expr.clone(), spec.env);
            self.spawner.spawn_blocking(move || {
                let frames = frames
                    .into_iter()
                    .map(|(name, t, scan)| {
                        let result =
                            scan.ok_or_else(|| "no longer cached".to_string())
                                .and_then(|scan| {
                                    let time = chrono::DateTime::from_timestamp(t, 0)
                                        .ok_or_else(|| "no scan time".to_string())?;
                                    column_product::evaluate_scan(&scan, &expr, &env, time)
                                        .map(|p| p.field)
                                        .map_err(|e| e.to_string())
                                });
                        (name, t, result)
                    })
                    .collect();
                let _ = tx.send(OverlayDelivery::Immediate(OverlayMsg::ColumnTrail(
                    Box::new(ColumnTrailDelivery { key, job, frames }),
                )));
                ctx.request_repaint();
            });
        }
        if changed {
            self.refresh_column_trail();
        }
    }

    /// Take a worker's volumes into the trail when they answer the current key and job.
    pub(super) fn accept_column_trail(&mut self, delivery: ColumnTrailDelivery) {
        let Some(state) = self.column_trail.as_mut() else {
            return;
        };
        if state.key != delivery.key || state.inflight != Some(delivery.job) {
            return;
        }
        state.inflight = None;
        for (name, t, result) in delivery.frames {
            match result {
                Ok(field) => match state.trail.push(t, field) {
                    GridMerge::Merged => {}
                    GridMerge::Reset(_) => state.skipped.clear(),
                    GridMerge::Skipped(_) => {
                        state.skipped.insert(name, "another grid".into());
                    }
                },
                Err(e) => {
                    state.skipped.insert(name, e);
                }
            }
        }
        self.refresh_column_trail();
    }

    /// Recompute the shown trail at the anchor and stage its upload.
    fn refresh_column_trail(&mut self) {
        let Some(state) = self.column_trail.as_mut() else {
            return;
        };
        let Some(anchor) = state.anchor else {
            return;
        };
        state.shown = state.trail.at(anchor);
        let Some(shown) = state.shown.as_ref() else {
            return;
        };
        let values: Vec<Option<f32>> = shown
            .field
            .values
            .iter()
            .map(|v| v.is_finite().then_some(*v))
            .collect();
        // The product's own range when it has one, else what the trail came out in.
        let Some(range) = wxdata::udp_volume::auto_range(values.iter(), state.spec_range) else {
            state.range = None;
            return;
        };
        state.table = state
            .spec_table
            .clone()
            .unwrap_or_else(|| crate::colormap::ramp_table(range.0, range.1));
        state.range = Some(range);
        let upload = column_product::column_upload(&shown.field, &state.table, range);
        let field = shown.field.clone();
        if let Some(fs) = self
            .fields
            .get_mut(&crate::render::FieldLayer::UserColumnTrail)
        {
            fs.stage(field, None, upload);
        }
    }

    /// Whether pane `idx` may draw the resident trail texture: its own product, window and
    /// playhead are the trail's.
    pub(crate) fn column_trail_ready(&self, idx: usize) -> bool {
        let Some(state) = self.column_trail.as_ref() else {
            return false;
        };
        let Some(Ok(spec)) = self.column_spec(idx) else {
            return false;
        };
        state.shown.is_some()
            && state.key == self.column_trail_key(idx, &spec)
            && self
                .column_trail_window(idx, i64::from(self.filters.trail_window_min) * 60)
                .is_some_and(|(anchor, _)| state.anchor == Some(anchor))
            && self
                .fields
                .get(&crate::render::FieldLayer::UserColumnTrail)
                .is_some_and(|f| f.grid.is_some())
    }

    /// The trail pane `idx` is drawing, for its legend, probe and export.
    pub(crate) fn column_trail_shown(&self, idx: usize) -> Option<&ColumnTrailState> {
        self.column_trail_ready(idx)
            .then_some(self.column_trail.as_ref())
            .flatten()
    }

    /// The legend card: the product's table over the trail's range, with its window and coverage,
    /// or a line saying what it is waiting for.
    pub(crate) fn paint_column_trail_key(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        idx: usize,
        y: f32,
    ) -> f32 {
        let what = if self.filters.trail_keep_min {
            "min"
        } else {
            "max"
        };
        if let Some(s) = self.column_trail_shown(idx) {
            if let (Some(shown), Some(range)) = (&s.shown, s.range) {
                let c = &shown.coverage;
                let mut note = format!(
                    "{what} over {} min · {} volumes{}",
                    self.filters.trail_window_min,
                    c.frames,
                    trail_coverage_note(c)
                );
                if s.pending > 0 {
                    note.push_str(&format!(" · {} still building", s.pending));
                }
                let left_out = s.skipped.len();
                if left_out > 0 {
                    note.push_str(&format!(" · {left_out} left out"));
                }
                return crate::ui::legend::draw_table_card(
                    painter,
                    prect,
                    &format!("{} trail", s.name),
                    &s.units,
                    &s.table,
                    range,
                    Some(&note),
                    y,
                );
            }
        }
        let text = match self.column_spec(idx) {
            None => "Column product trail: choose a column product in User-defined products".into(),
            Some(Err(e)) => format!("Column product trail: {e}"),
            Some(Ok(_)) => "Column product trail: building from the loop's volumes…".into(),
        };
        crate::ui::legend::draw_status_card(painter, prect, &text, y)
    }

    /// The probe row: the trail's value at the cell, the scan that set it and the coverage.
    pub(super) fn column_trail_probe(
        &self,
        idx: usize,
        lon: f64,
        lat: f64,
    ) -> crate::ui::cursor_probe::ProbeRow {
        let state = self.column_trail_shown(idx);
        let shown = state.and_then(|s| s.shown.as_ref());
        let tz = self.active_tz();
        crate::ui::cursor_probe::ProbeRow {
            pane: idx,
            source: "Local radar (column product trail)".into(),
            product: state.map_or_else(
                || "Column product trail".into(),
                |s| format!("{} trail {} min", s.name, self.filters.trail_window_min),
            ),
            time: shown.map(|w| w.field.time),
            value: state.zip(shown).and_then(|(s, w)| {
                let (v, who) = w.at_point(lon, lat)?;
                Some(match (v, who) {
                    (Some(v), Some(t)) => format!(
                        "{v:.2} {} · from the {} volume",
                        s.units,
                        chrono::DateTime::from_timestamp(t, 0)
                            .map_or_else(String::new, |t| crate::timefmt::fmt_clock(t, tz, true))
                    )
                    .replace("  ", " "),
                    _ => "\u{2014}".into(),
                })
            }),
            folded: false,
        }
    }
}

/// The trail's coverage in a few words (shared wording with the radar trail's status line).
fn trail_coverage_note(c: &wxdata::extrema::Coverage) -> String {
    let mut out = String::new();
    if c.short_s > 0 {
        out.push_str(&format!(
            " · history covers {} of {} min",
            ((c.requested_s - c.short_s) / 60).max(0),
            c.requested_s / 60
        ));
    }
    if c.missing > 0 {
        out.push_str(&format!(" · {} missing", c.missing));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use column_product::ColumnSpec;

    fn mayfield() -> Arc<Scan> {
        Arc::new(
            wxdata::level2::decode_volume(
                include_bytes!("../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                    .to_vec(),
            )
            .unwrap(),
        )
    }

    fn spec(src: &str, h0: Option<f32>) -> ColumnSpec {
        ColumnSpec {
            name: "p".into(),
            units: String::new(),
            expr: wxdata::udp::parse(src).unwrap(),
            range: None,
            table: None,
            env: wxdata::udp_column::ColumnEnv {
                antenna_altitude_m: Some(150.0),
                levels: wxdata::udp_column::Levels {
                    h0_m: h0,
                    ..Default::default()
                },
            },
            env_source: Some("test level".into()),
        }
    }

    /// A trail frame is the product the map shows for that volume, bit for bit.
    #[test]
    fn a_trail_frame_is_the_single_volume_product() {
        let scan = mayfield();
        let time = chrono::DateTime::from_timestamp(1_639_193_029, 0).unwrap();
        let s = spec("max_vertical(REF, REF >= 20) - min_vertical(REF)", None);
        let frame = column_product::evaluate_scan(&scan, &s.expr, &s.env, time).unwrap();
        let mut vol = Volume::from_live(Arc::clone(&scan), "KPAH".into(), time);
        let single = column_product::single_volume_product(&mut vol, &s, time).unwrap();
        assert_eq!(frame.field.time, time);
        assert!(frame.cells_with_value > 100);
        assert!(frame.field.values.iter().map(|v| v.to_bits()).eq(single
            .field
            .values
            .iter()
            .map(|v| v.to_bits())));
    }

    #[test]
    fn the_trail_identity_follows_the_definition_and_the_environment_it_reads() {
        let settings = Settings::default();
        let id = |s: &ColumnSpec| column_product::product_identity(&settings, 0, s);
        let plain = spec("max_vertical(REF)", Some(3000.0));
        assert_eq!(id(&plain), id(&spec("max_vertical(REF)", Some(4000.0))));
        let env = spec(
            "max_vertical(ZDR, BEAM_ALTITUDE_M > FREEZING_LEVEL_M)",
            Some(3000.0),
        );
        assert_ne!(
            id(&env),
            id(&spec(
                "max_vertical(ZDR, BEAM_ALTITUDE_M > FREEZING_LEVEL_M)",
                Some(3100.0)
            ))
        );
        let renamed = ColumnSpec {
            name: "q".into(),
            ..plain.clone()
        };
        assert_ne!(id(&plain), id(&renamed));
    }
}
