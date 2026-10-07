//! Any field a regional model file holds (ROADMAP_PARITY M5.3): the run's own `.idx` inventory
//! read into fields (`wxdata::model_inventory`), browsed by quantity and level, and the picked one
//! drawn as the pane's [`FieldLayer::ModelField`](crate::render::FieldLayer::ModelField).
//!
//! Only parameters whose units are vetted can be picked; the rest are listed with the reason. The
//! pick keeps its timing kind (instant, an N-hour window, or since the run began), so scrubbing
//! the lead fetches the same kind of field at the new lead, never another interval's.
use super::*;
use chrono::Timelike;
use wxdata::hrrr::Model as Regional;
use wxdata::model_inventory::{Field, TimingKind};

/// Where an inventory comes from: a regional model's file, or the GFS's quarter-degree file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InventorySource {
    Regional(Regional),
    Gfs,
}

/// The models whose inventories can be browsed: every regional model the app reads, the HRRR's
/// pressure-level file for its mandatory levels, and the GFS.
pub(crate) const BROWSABLE: [InventorySource; 7] = [
    InventorySource::Regional(Regional::Hrrr),
    InventorySource::Regional(Regional::HrrrPressure),
    InventorySource::Regional(Regional::Rap),
    InventorySource::Regional(Regional::NamNest),
    InventorySource::Regional(Regional::Nam),
    InventorySource::Regional(Regional::Nbm),
    InventorySource::Gfs,
];

impl Default for InventorySource {
    fn default() -> Self {
        InventorySource::Regional(Regional::Hrrr)
    }
}

impl InventorySource {
    /// The name the stamp carries as its source.
    pub(crate) fn source_id(self) -> &'static str {
        match self {
            InventorySource::Regional(m) => m.label(),
            InventorySource::Gfs => wxdata::global::GlobalModel::Gfs.label(),
        }
    }

    /// The hours between cycles, for which pinned runs apply.
    pub(crate) fn cycle_hours(self) -> u32 {
        match self {
            InventorySource::Regional(m) => m.def().cycle_hours,
            InventorySource::Gfs => 6,
        }
    }

    /// The pane's lead for this source: the regional hour, or the global one.
    pub(crate) fn lead(self, models: &crate::model_pane::ModelControls) -> u16 {
        match self {
            InventorySource::Regional(_) => u16::from(models.hrrr_fcst_hour),
            InventorySource::Gfs => models.global_fcst_hour,
        }
    }
}

pub(crate) fn model_label(m: InventorySource) -> &'static str {
    match m {
        InventorySource::Regional(Regional::HrrrPressure) => "HRRR pressure levels",
        InventorySource::Regional(other) => other.label(),
        InventorySource::Gfs => "GFS (0.25\u{b0})",
    }
}

/// A picked field as a pane keeps it (in `ModelControls`, so it saves with the workspace).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedFieldPick {
    pub model: InventorySource,
    /// The NCEP abbreviation and level text exactly as the inventory names them.
    pub var: String,
    pub level: String,
    pub kind: TimingKind,
}

/// [`SavedFieldPick`] in the `Copy` form a request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FieldPick {
    pub model: InventorySource,
    pub var: &'static str,
    pub level: &'static str,
    pub kind: TimingKind,
}

impl SavedFieldPick {
    pub(crate) fn pick(&self) -> FieldPick {
        FieldPick {
            model: self.model,
            var: intern(&self.var),
            level: intern(&self.level),
            kind: self.kind,
        }
    }

    pub(crate) fn of(model: InventorySource, field: &Field) -> Option<Self> {
        Some(Self {
            model,
            var: field.entry.var.clone(),
            level: field.entry.level_text.clone(),
            kind: field.entry.timing.kind()?,
        })
    }
}

impl FieldPick {
    /// "Temperature · 500 hPa · instant (°C)".
    pub(crate) fn label(self) -> String {
        let level = wxdata::model_inventory::Level::parse(self.level).label();
        match wxdata::model_inventory::vetted(self.var) {
            Some(q) => format!(
                "{} \u{b7} {level} \u{b7} {} ({})",
                q.name,
                self.kind.label(),
                q.unit
            ),
            None => format!("{} \u{b7} {level}", self.var),
        }
    }

    /// The stamp's product id: what was asked for, exactly.
    pub(crate) fn product_id(self) -> String {
        format!("{}:{}:{}", self.var, self.level, self.kind.label())
    }
}

/// A `'static` copy of `s`, made once per distinct string: parameter abbreviations and level
/// texts, a few hundred at most, so a request can stay `Copy`.
pub(crate) fn intern(s: &str) -> &'static str {
    static SEEN: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> =
        std::sync::Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let set = seen.get_or_insert_with(Default::default);
    if let Some(&s) = set.get(s) {
        return s;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// The range a discovered field is coloured over: its 2nd to 98th percentile, so one spike does
/// not wash the map out. `None` without finite values.
pub(crate) fn field_range(f: &wxdata::mrms::MrmsField) -> Option<(f32, f32)> {
    let mut v: Vec<f32> = f.values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    let (lo, hi) = (at(0.02), at(0.98));
    Some(if hi > lo {
        (lo, hi)
    } else {
        (lo - 0.5, lo + 0.5)
    })
}

/// The GPU upload: a ramp over the field's own range, values outside clamped to its ends.
pub(crate) fn model_field_upload(f: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
    let range = field_range(f).unwrap_or((0.0, 1.0));
    super::column_product::column_upload(f, &crate::colormap::ramp_table(range.0, range.1), range)
}

/// One model inventory lookup.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InventoryState {
    Pending,
    Ready {
        run: DateTime<Utc>,
        lead_h: u16,
        fields: Vec<Field>,
    },
    Failed(String),
}

type InventoryKey = (InventorySource, Option<DateTime<Utc>>, u16);

#[derive(Default)]
pub(crate) struct ModelFieldBrowser {
    pub open: bool,
    pub model: InventorySource,
    pub search: String,
    key: Option<InventoryKey>,
    pub state: Option<InventoryState>,
    rx: Option<std::sync::mpsc::Receiver<(InventoryKey, InventoryState)>>,
}

impl HookEchoApp {
    /// Look up the inventory for the browser's model at the active pane's run and lead, once per
    /// context; an answer for an older context is dropped.
    fn sync_model_inventory(&mut self, ctx: &egui::Context) {
        let models = &self.views[self.active].models;
        let source = self.field_browser.model;
        let key = (
            source,
            models
                .model_run
                .filter(|r| r.hour() % source.cycle_hours() == 0),
            source.lead(models),
        );
        if let Some(rx) = &self.field_browser.rx {
            while let Ok((k, state)) = rx.try_recv() {
                if Some(k) == self.field_browser.key {
                    self.field_browser.state = Some(state);
                }
            }
        }
        if self.field_browser.key == Some(key) {
            return;
        }
        self.field_browser.key = Some(key);
        self.field_browser.state = Some(InventoryState::Pending);
        let (tx, rx) = std::sync::mpsc::channel();
        self.field_browser.rx = Some(rx);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let got = match key.0 {
                InventorySource::Regional(m) => {
                    let lead = key.2.min(u16::from(u8::MAX)) as u8;
                    wxdata::hrrr::fetch_inventory(&http, m, key.1, lead)
                        .await
                        .map(|(run, lead, fields)| (run, u16::from(lead), fields))
                }
                InventorySource::Gfs => wxdata::global::fetch_gfs_inventory(&http, key.1, key.2)
                    .await
                    .map(|(run, fields)| (run, key.2, fields)),
            };
            let state = match got {
                Ok((run, lead_h, fields)) => InventoryState::Ready {
                    run,
                    lead_h,
                    fields,
                },
                Err(e) => InventoryState::Failed(format!("{e:#}")),
            };
            let _ = tx.send((key, state));
            ctx.request_repaint();
        });
    }

    /// Put `pick` on the active pane and turn its layer on.
    pub(crate) fn show_model_field(&mut self, pick: SavedFieldPick) {
        let v = &mut self.views[self.active];
        v.models.field = Some(pick);
        v.fields_on.insert(crate::render::FieldLayer::ModelField);
        v.model_playback.active = true;
    }

    /// The browser window: model, a search box, the vetted fields by quantity and level (click to
    /// show; hover says whether one compares with the shown field), and the rest with reasons.
    pub(crate) fn model_fields_window(&mut self, ctx: &egui::Context) {
        if !self.field_browser.open {
            return;
        }
        self.sync_model_inventory(ctx);
        let mut open = true;
        let mut chosen = None;
        let current = self.views[self.active].models.field.clone();
        let lead = self
            .field_browser
            .model
            .lead(&self.views[self.active].models);
        egui::Window::new("Model fields")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                let b = &mut self.field_browser;
                ui.horizontal_wrapped(|ui| {
                    ui.label("Model");
                    egui::ComboBox::from_id_salt("model_fields_model")
                        .selected_text(model_label(b.model))
                        .show_ui(ui, |ui| {
                            for m in BROWSABLE {
                                ui.selectable_value(&mut b.model, m, model_label(m));
                            }
                        });
                    ui.label(format!("F+{lead}h (the pane's lead)"));
                });
                ui.add(
                    egui::TextEdit::singleline(&mut b.search)
                        .hint_text("Search: temperature, 500, wind…"),
                );
                let fields = match &b.state {
                    None | Some(InventoryState::Pending) => {
                        ui.weak("Reading the run's inventory\u{2026}");
                        return;
                    }
                    Some(InventoryState::Failed(e)) => {
                        ui.colored_label(egui::Color32::from_rgb(230, 120, 80), e);
                        return;
                    }
                    Some(InventoryState::Ready {
                        run,
                        lead_h,
                        fields,
                    }) => {
                        ui.weak(format!(
                            "{} {}Z run, F+{lead_h}h: {} fields, {} with vetted units",
                            model_label(b.model),
                            run.format("%Y-%m-%d %H"),
                            fields.len(),
                            fields.iter().filter(|f| f.supported()).count()
                        ));
                        fields
                    }
                };
                let needle = b.search.trim().to_lowercase();
                let matches =
                    |f: &Field| needle.is_empty() || f.label().to_lowercase().contains(&needle);
                let shown = current.as_ref().and_then(|c| {
                    fields.iter().find(|f| {
                        c.model == b.model
                            && f.entry.var == c.var
                            && f.entry.level_text == c.level
                            && f.entry.timing.kind() == Some(c.kind)
                    })
                });
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for f in fields.iter().filter(|f| f.supported() && matches(f)) {
                            let on = shown.is_some_and(|s| std::ptr::eq(s, f));
                            let mut resp = ui.selectable_label(on, f.label());
                            if let Some(s) = shown.filter(|_| !on) {
                                resp = resp.on_hover_text(match s.comparable(f) {
                                    Ok(()) => "Comparable with the shown field".to_string(),
                                    Err(why) => {
                                        format!("Not comparable with the shown field: {why}")
                                    }
                                });
                            }
                            if resp.clicked() {
                                chosen = SavedFieldPick::of(b.model, f);
                            }
                        }
                        let pairs = wxdata::model_inventory::vector_pairs(fields);
                        if !pairs.is_empty() {
                            ui.weak(format!(
                            "Wind components pair at {} levels (shown as components; the HRRR's \
                             and NAM's are grid-relative)",
                            pairs.len()
                        ));
                        }
                        let unsupported: Vec<&Field> = fields
                            .iter()
                            .filter(|f| !f.supported() && matches(f))
                            .collect();
                        egui::CollapsingHeader::new(format!("Not shown ({})", unsupported.len()))
                            .id_salt("model_fields_unsupported")
                            .show(ui, |ui| {
                                for f in unsupported {
                                    ui.weak(format!(
                                        "{} \u{2014} {}",
                                        f.label(),
                                        f.unsupported_reason().unwrap_or_default()
                                    ));
                                }
                            });
                    });
            });
        self.field_browser.open = open;
        if let Some(pick) = chosen {
            self.show_model_field(pick);
        }
    }

    /// The legend card for the pane's model field: its quantity, level and timing, the run and
    /// lead, over the range it is coloured across.
    pub(crate) fn paint_model_field_key(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        idx: usize,
        y: f32,
    ) -> f32 {
        let Some(pick) = self.views[idx]
            .models
            .field
            .as_ref()
            .map(SavedFieldPick::pick)
        else {
            return 0.0;
        };
        let state = self.field_state_for(idx, crate::render::FieldLayer::ModelField);
        let Some(range) = state.and_then(|s| s.grid.as_ref()).and_then(field_range) else {
            return crate::ui::legend::draw_status_card(
                painter,
                prect,
                &format!("{}: loading\u{2026}", pick.label()),
                y,
            );
        };
        let unit = wxdata::model_inventory::vetted(pick.var).map_or("", |q| q.unit);
        let note = state.and_then(|s| s.stamp.as_ref()).map(|s| {
            format!(
                "{} run {} \u{b7} valid {} UTC \u{b7} 2nd\u{2013}98th percentile",
                model_label(pick.model),
                s.run_time
                    .map_or_else(|| "?".into(), |r| r.format("%HZ").to_string()),
                s.valid_time.format("%m-%d %H:%M")
            )
        });
        let title = pick.label();
        crate::ui::legend::draw_table_card(
            painter,
            prect,
            title.split(" (").next().unwrap_or(&title),
            unit,
            &crate::colormap::ramp_table(range.0, range.1),
            range,
            note.as_deref(),
            y,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_survives_saving_and_names_what_it_shows() {
        let saved = SavedFieldPick {
            model: InventorySource::Regional(Regional::HrrrPressure),
            var: "TMP".into(),
            level: "500 mb".into(),
            kind: TimingKind::Instant,
        };
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(
            serde_json::from_str::<SavedFieldPick>(&json).unwrap(),
            saved
        );
        let p = saved.pick();
        assert_eq!(p.label(), "Temperature \u{b7} 500 hPa \u{b7} instant (°C)");
        assert_eq!(p.product_id(), "TMP:500 mb:instant");
        // Interning gives the same pointer for the same text.
        assert!(std::ptr::eq(intern("500 mb"), p.level));
    }

    fn grid(values: Vec<f32>) -> wxdata::mrms::MrmsField {
        wxdata::mrms::MrmsField {
            nx: values.len(),
            ny: 1,
            values,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 40.0,
            lat_south: 30.0,
            time: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_fetched_field_is_accepted_only_by_the_request_that_asked_for_it() {
        use super::super::model_context::ModelRequest;
        let run = chrono::Utc::now()
            .with_minute(0)
            .and_then(|t| t.with_second(0))
            .and_then(|t| t.with_nanosecond(0))
            .unwrap();
        let pick = SavedFieldPick {
            model: InventorySource::Regional(Regional::HrrrPressure),
            var: "TMP".into(),
            level: "500 mb".into(),
            kind: TimingKind::Instant,
        }
        .pick();
        let valid = run + chrono::Duration::hours(6);
        let mut field = grid(vec![-20.0; 4]);
        field.time = valid;
        // As the fetch stamps it.
        let stamped = super::super::field_state::model_field(
            pick.model.source_id(),
            &pick.product_id(),
            field,
            Some(run),
            valid,
            false,
        )
        .unwrap();
        let asked = ModelRequest::Discovered(pick, 6, Some(run));
        assert!(asked.accepts(&stamped.stamp));
        assert!(
            asked.description().contains("Temperature"),
            "{}",
            asked.description()
        );
        // Another level, lead or run is a different request.
        let other_level = FieldPick {
            level: intern("700 mb"),
            ..pick
        };
        assert!(!ModelRequest::Discovered(other_level, 6, Some(run)).accepts(&stamped.stamp));
        assert!(!ModelRequest::Discovered(pick, 7, Some(run)).accepts(&stamped.stamp));
        let earlier = run - chrono::Duration::hours(1);
        assert!(!ModelRequest::Discovered(pick, 6, Some(earlier)).accepts(&stamped.stamp));
        // The same field from the GFS is another source: not interchangeable.
        let gfs = FieldPick {
            model: InventorySource::Gfs,
            ..pick
        };
        assert!(!ModelRequest::Discovered(gfs, 6, Some(run)).accepts(&stamped.stamp));
        let mut gfs_field = grid(vec![-20.0; 4]);
        let gfs_run = run - chrono::Duration::hours(i64::from(run.hour() % 6));
        gfs_field.time = gfs_run + chrono::Duration::hours(120);
        let gfs_stamped = super::super::field_state::model_field(
            gfs.model.source_id(),
            &gfs.product_id(),
            gfs_field,
            Some(gfs_run),
            gfs_run + chrono::Duration::hours(120),
            false,
        )
        .unwrap();
        assert!(
            ModelRequest::Discovered(gfs, 120, Some(gfs_run)).accepts(&gfs_stamped.stamp),
            "a GFS lead past a regional model's u8 range"
        );
    }

    #[test]
    fn the_range_skips_spikes_and_missing_cells() {
        let mut values: Vec<f32> = (0..100).map(|i| i as f32).collect();
        values[50] = f32::NAN;
        values[99] = 10_000.0;
        let f = grid(values);
        let (lo, hi) = field_range(&f).unwrap();
        assert!(lo <= 3.0 && (95.0..=98.0).contains(&hi), "{lo}..{hi}");
        let flat = grid(vec![5.0; 10]);
        assert_eq!(field_range(&flat), Some((4.5, 5.5)));
        let empty = grid(vec![f32::NAN; 4]);
        assert_eq!(field_range(&empty), None);
    }
}
