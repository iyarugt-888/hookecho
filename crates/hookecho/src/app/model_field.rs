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

/// Where an inventory comes from: a regional model's file, or a global model's quarter-degree
/// file (the GFS's `.idx`, ECMWF's `.index`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InventorySource {
    Regional(Regional),
    Gfs,
    Ecmwf,
}

/// The models whose inventories can be browsed: every regional model the app reads, the HRRR's
/// pressure-level file for its mandatory levels, the GFS and the ECMWF IFS.
pub(crate) const BROWSABLE: [InventorySource; 8] = [
    InventorySource::Regional(Regional::Hrrr),
    InventorySource::Regional(Regional::HrrrPressure),
    InventorySource::Regional(Regional::Rap),
    InventorySource::Regional(Regional::NamNest),
    InventorySource::Regional(Regional::Nam),
    InventorySource::Regional(Regional::Nbm),
    InventorySource::Gfs,
    InventorySource::Ecmwf,
];

impl Default for InventorySource {
    fn default() -> Self {
        InventorySource::Regional(Regional::Hrrr)
    }
}

impl InventorySource {
    /// The global model, for a global source.
    pub(crate) fn global(self) -> Option<wxdata::global::GlobalModel> {
        match self {
            InventorySource::Regional(_) => None,
            InventorySource::Gfs => Some(wxdata::global::GlobalModel::Gfs),
            InventorySource::Ecmwf => Some(wxdata::global::GlobalModel::Ecmwf),
        }
    }

    /// The name the stamp carries as its source.
    pub(crate) fn source_id(self) -> &'static str {
        match (self, self.global()) {
            (InventorySource::Regional(m), _) => m.label(),
            (_, Some(g)) => g.label(),
            (_, None) => unreachable!("a non-regional source is global"),
        }
    }

    /// The hours between cycles, for which pinned runs apply.
    pub(crate) fn cycle_hours(self) -> u32 {
        match self {
            InventorySource::Regional(m) => m.def().cycle_hours,
            InventorySource::Gfs | InventorySource::Ecmwf => 6,
        }
    }

    /// The pane's lead for this source: the regional hour, or the global one snapped down to a
    /// lead the model publishes a file for.
    pub(crate) fn lead(self, models: &crate::model_pane::ModelControls) -> u16 {
        match (self, self.global()) {
            (InventorySource::Regional(_), _) => u16::from(models.hrrr_fcst_hour),
            (_, Some(g)) => g.inventory_lead(models.global_fcst_hour),
            (_, None) => unreachable!("a non-regional source is global"),
        }
    }
}

pub(crate) fn model_label(m: InventorySource) -> &'static str {
    match m {
        InventorySource::Regional(Regional::HrrrPressure) => "HRRR pressure levels",
        InventorySource::Regional(other) => other.label(),
        InventorySource::Gfs => "GFS (0.25\u{b0})",
        InventorySource::Ecmwf => "ECMWF IFS (0.25\u{b0})",
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
    /// The level's wind as a vector: `var` is its eastward component, the pair is fetched
    /// together, shown as speed in knots and drawn as barbs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vector: bool,
    /// Show this field minus the same field from another model (1008.md E3): fetched at the
    /// same run and lead, refused unless both are valid at the same instant. Scalars only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minus: Option<InventorySource>,
}

/// [`SavedFieldPick`] in the `Copy` form a request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FieldPick {
    pub model: InventorySource,
    pub var: &'static str,
    pub level: &'static str,
    pub kind: TimingKind,
    pub vector: bool,
    pub minus: Option<InventorySource>,
}

impl SavedFieldPick {
    pub(crate) fn pick(&self) -> FieldPick {
        FieldPick {
            model: self.model,
            var: intern(&self.var),
            level: intern(&self.level),
            kind: self.kind,
            vector: self.vector,
            minus: self.minus,
        }
    }

    /// The wind at a `(u, v)` pair's level, as one vector pick.
    pub(crate) fn wind(model: InventorySource, u: &Field) -> Option<Self> {
        Some(Self {
            vector: true,
            ..Self::of(model, u)?
        })
    }

    pub(crate) fn of(model: InventorySource, field: &Field) -> Option<Self> {
        Some(Self {
            model,
            var: field.entry.var.clone(),
            level: field.entry.level_text.clone(),
            kind: field.entry.timing.kind()?,
            vector: false,
            minus: None,
        })
    }
}

impl FieldPick {
    /// "Temperature · 500 hPa · instant (°C)".
    pub(crate) fn label(self) -> String {
        let level = wxdata::model_inventory::Level::parse(self.level).label();
        if self.vector {
            return format!("Wind \u{b7} {level} \u{b7} {} (kt)", self.kind.label());
        }
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

    /// The unit the shown grid is in.
    pub(crate) fn unit(self) -> &'static str {
        if self.vector {
            return "kt";
        }
        wxdata::model_inventory::vetted(self.var).map_or("", |q| q.unit)
    }

    /// The stamp's product id: what was asked for, exactly. A difference names the model it
    /// subtracts, so it is never taken for the field itself.
    pub(crate) fn product_id(self) -> String {
        let var = if self.vector { "WIND" } else { self.var };
        let base = format!("{var}:{}:{}", self.level, self.kind.label());
        match self.minus {
            Some(m) => format!("{base}:minus:{}", m.source_id()),
            None => base,
        }
    }

    /// "GFS − ECMWF" for a difference; `None` for the field itself.
    pub(crate) fn difference_label(self) -> Option<String> {
        self.minus
            .map(|m| format!("{} \u{2212} {}", model_label(self.model), model_label(m)))
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

/// The symmetric range a difference is coloured over: the larger of its 2nd and 98th
/// percentiles' magnitudes, so zero is the middle colour. `None` without finite values.
pub(crate) fn difference_range(f: &wxdata::mrms::MrmsField) -> Option<f32> {
    let (lo, hi) = field_range(f)?;
    Some(lo.abs().max(hi.abs()).max(1e-3))
}

/// A diverging table over `-m..m`: blue below zero, white at zero, red above.
pub(crate) fn difference_table(m: f32) -> crate::colormap::ColorTable {
    let pal = format!(
        "Color: {} 40 70 200\nColor: {} 140 170 240\nColor: 0 245 245 245\nColor: {} 240 150 120\nColor: {} 200 40 30\n",
        -m,
        -m / 2.0,
        m / 2.0,
        m
    );
    crate::colormap::parse_pal(&pal).expect("a generated table parses")
}

/// The GPU upload for a difference: the diverging table over its symmetric range.
pub(crate) fn model_difference_upload(f: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
    let m = difference_range(f).unwrap_or(1.0);
    super::column_product::column_upload(f, &difference_table(m), (-m, m))
}

/// The GPU upload: a ramp over the field's own range, values outside clamped to its ends.
pub(crate) fn model_field_upload(f: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
    let range = field_range(f).unwrap_or((0.0, 1.0));
    super::column_product::column_upload(f, &crate::colormap::ramp_table(range.0, range.1), range)
}

/// A browsed wind's east and north components (m/s) on one lattice.
pub(crate) type WindPair = (wxdata::mrms::MrmsField, wxdata::mrms::MrmsField);

/// A browsed wind as barb points for an export (1008.md E3): an `n`×`n` lattice over `bounds`
/// (`min_lon, min_lat, max_lon, max_lat`), each point with the wind's speed (knots and m/s) and
/// the direction it blows from, read as the barbs and the probe read it. Points outside the grid,
/// with a missing component, or calm are left out rather than given a direction.
pub(crate) fn wind_barb_features(
    wind: &WindPair,
    bounds: (f64, f64, f64, f64),
    n: usize,
    valid: chrono::DateTime<chrono::Utc>,
    source: &str,
) -> Vec<wxdata::gis::GisFeature> {
    let (lon0, lat0, lon1, lat1) = bounds;
    let n = n.max(1);
    let at = |i: usize, a: f64, b: f64| a + (b - a) * (i as f64 + 0.5) / n as f64;
    let mut out = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let (lon, lat) = (at(x, lon0, lon1), at(y, lat0, lat1));
            let (Some(u), Some(v)) = (
                wind.0.sample_bilinear(lon, lat),
                wind.1.sample_bilinear(lon, lat),
            ) else {
                continue;
            };
            let Some(from) = wind_from_deg(u, v) else {
                continue;
            };
            let speed = f64::from(u).hypot(f64::from(v));
            let mut properties = serde_json::Map::new();
            properties.insert("speed_kt".into(), (speed * 1.943_844).into());
            properties.insert("speed_ms".into(), speed.into());
            properties.insert("from_deg".into(), from.into());
            properties.insert("valid_utc".into(), valid.to_rfc3339().into());
            properties.insert("source".into(), source.into());
            out.push(wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Point([lon, lat]),
                properties,
            });
        }
    }
    out
}

/// The meteorological direction a wind of east/north components `(u, v)` blows from, degrees
/// clockwise from north in `0..360`; `None` for a missing component or calm air, which has no
/// direction.
pub(crate) fn wind_from_deg(u: f32, v: f32) -> Option<f64> {
    if !(u.is_finite() && v.is_finite()) || (u == 0.0 && v == 0.0) {
        return None;
    }
    Some(
        f64::from(-u)
            .atan2(f64::from(-v))
            .to_degrees()
            .rem_euclid(360.0),
    )
}

/// Whether two grids cover the same cells: what pairs a wind's components with its speed.
pub(crate) fn same_lattice(a: &wxdata::mrms::MrmsField, b: &wxdata::mrms::MrmsField) -> bool {
    a.nx == b.nx
        && a.ny == b.ny
        && a.values.len() == b.values.len()
        && a.lon_west == b.lon_west
        && a.lon_east == b.lon_east
        && a.lat_north == b.lat_north
        && a.lat_south == b.lat_south
        && a.time == b.time
}

/// `f` on `like`'s lattice (same extent, coarser or equal cells) by nearest cell: display
/// decimation pools the speed by its maximum, which would bias a signed component.
pub(crate) fn resample_like(
    f: wxdata::mrms::MrmsField,
    like: &wxdata::mrms::MrmsField,
) -> wxdata::mrms::MrmsField {
    if (f.nx, f.ny) == (like.nx, like.ny) || like.nx == 0 || like.ny == 0 {
        return f;
    }
    let mut values = Vec::with_capacity(like.nx * like.ny);
    for y in 0..like.ny {
        let sy = (((y as f64 + 0.5) * f.ny as f64 / like.ny as f64) as usize).min(f.ny - 1);
        for x in 0..like.nx {
            let sx = (((x as f64 + 0.5) * f.nx as f64 / like.nx as f64) as usize).min(f.nx - 1);
            values.push(f.values.get(sy * f.nx + sx).copied().unwrap_or(f32::NAN));
        }
    }
    wxdata::mrms::MrmsField {
        values,
        nx: like.nx,
        ny: like.ny,
        ..f
    }
}

const MS_TO_KT: f32 = 1.943_844;

/// The wind speed in knots from east/north components in m/s; a cell missing either component
/// stays missing.
pub(crate) fn wind_speed_kt(
    u: &wxdata::mrms::MrmsField,
    v: &wxdata::mrms::MrmsField,
) -> anyhow::Result<wxdata::mrms::MrmsField> {
    anyhow::ensure!(same_lattice(u, v), "wind components on different grids");
    let values = u
        .values
        .iter()
        .zip(&v.values)
        .map(|(&a, &b)| {
            if a.is_finite() && b.is_finite() {
                a.hypot(b) * MS_TO_KT
            } else {
                f32::NEG_INFINITY
            }
        })
        .collect();
    Ok(wxdata::mrms::MrmsField {
        values,
        ..u.clone()
    })
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
                InventorySource::Gfs | InventorySource::Ecmwf => {
                    let model = key.0.global().expect("a global source");
                    wxdata::global::fetch_global_inventory(&http, model, key.1, key.2)
                        .await
                        .map(|(run, fields)| (run, key.2, fields))
                }
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

    /// The components of the wind pane `idx` shows from the browser, once its speed is the
    /// staged, accepted field for the pane's exact request.
    pub(crate) fn model_wind_for(&self, idx: usize) -> Option<std::sync::Arc<WindPair>> {
        let layer = crate::render::FieldLayer::ModelField;
        if !self.views.get(idx)?.fields_on.contains(&layer) {
            return None;
        }
        let request = self.selected_model_request_for(idx, layer)?;
        let slot = self.model_fields.get(&request)?;
        slot.state
            .model_ready(request)
            .then(|| slot.vectors.clone())?
    }

    /// The direction the browsed wind on pane `idx` blows from at `(lon, lat)`, degrees clockwise
    /// from north (1008.md E3): read from the same east/north components, sampled the same way,
    /// as the barbs drawn there. `None` without a wind pick or outside its grid.
    pub(crate) fn model_wind_from_deg(&self, idx: usize, lon: f64, lat: f64) -> Option<f64> {
        let wind = self.model_wind_for(idx)?;
        let (u, v) = (
            wind.0.sample_bilinear(lon, lat)?,
            wind.1.sample_bilinear(lon, lat)?,
        );
        wind_from_deg(u, v)
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
                            && !c.vector
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
                            // The shown field, from another model: offer its difference.
                            if let Some(c) = current.as_ref().filter(|c| {
                                c.model != b.model
                                    && !c.vector
                                    && f.entry.var == c.var
                                    && f.entry.level_text == c.level
                                    && f.entry.timing.kind() == Some(c.kind)
                            }) {
                                let on = c.minus == Some(b.model);
                                if ui
                                    .selectable_label(
                                        on,
                                        format!(
                                            "    \u{21b3} show {} \u{2212} {}",
                                            model_label(c.model),
                                            model_label(b.model)
                                        ),
                                    )
                                    .on_hover_text(
                                        "The shown field minus this model's, at the same run and \
                                         lead; refused unless both are valid at the same time",
                                    )
                                    .clicked()
                                {
                                    chosen = Some(SavedFieldPick {
                                        minus: (!on).then_some(b.model),
                                        ..c.clone()
                                    });
                                }
                            }
                        }
                        let pairs = wxdata::model_inventory::vector_pairs(fields);
                        let winds: Vec<SavedFieldPick> = pairs
                            .iter()
                            .filter_map(|(u, _)| SavedFieldPick::wind(b.model, u))
                            .filter(|w| {
                                needle.is_empty()
                                    || w.pick().label().to_lowercase().contains(&needle)
                            })
                            .collect();
                        if !winds.is_empty() {
                            ui.separator();
                            ui.weak(
                                "Wind: both components turned to east/north, speed in knots \
                                 under barbs (a single component above is as published: \
                                 grid-relative on the HRRR, RAP and NAM)",
                            );
                        }
                        for w in winds {
                            let on = current.as_ref() == Some(&w);
                            if ui.selectable_label(on, w.pick().label()).clicked() {
                                chosen = Some(w);
                            }
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
        if let (Some(diff), Some(m)) = (
            pick.difference_label(),
            state
                .and_then(|s| s.grid.as_ref())
                .and_then(difference_range),
        ) {
            let note = state.and_then(|s| s.stamp.as_ref()).map(|s| {
                format!(
                    "same run {} and lead \u{b7} valid {} UTC \u{b7} symmetric 98th percentile",
                    s.run_time
                        .map_or_else(|| "?".into(), |r| r.format("%HZ").to_string()),
                    s.valid_time.format("%m-%d %H:%M")
                )
            });
            let title = pick.label();
            return crate::ui::legend::draw_table_card(
                painter,
                prect,
                &format!(
                    "{} \u{b7} {diff}",
                    title.split(" (").next().unwrap_or(&title)
                ),
                pick.unit(),
                &difference_table(m),
                (-m, m),
                note.as_deref(),
                y,
            );
        }
        let Some(range) = state.and_then(|s| s.grid.as_ref()).and_then(field_range) else {
            return crate::ui::legend::draw_status_card(
                painter,
                prect,
                &format!("{}: loading\u{2026}", pick.label()),
                y,
            );
        };
        let unit = pick.unit();
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
    fn a_wind_exports_as_barb_points_read_like_the_probe() {
        let t = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let grid = |value: f32| wxdata::mrms::MrmsField {
            values: vec![value; 4 * 4],
            nx: 4,
            ny: 4,
            lon_west: -100.0,
            lon_east: -96.0,
            lat_north: 38.0,
            lat_south: 34.0,
            time: t,
        };
        // A 10 m/s westerly over the whole grid.
        let wind: WindPair = (grid(10.0), grid(0.0));
        let points = wind_barb_features(&wind, (-99.0, 35.0, -97.0, 37.0), 3, t, "GFS 850 hPa");
        assert_eq!(points.len(), 9);
        for p in &points {
            let get = |k: &str| p.properties[k].as_f64().unwrap();
            assert!((get("from_deg") - 270.0).abs() < 1e-6);
            assert!((get("speed_ms") - 10.0).abs() < 1e-6);
            assert!((get("speed_kt") - 19.438_44).abs() < 1e-3);
            assert_eq!(p.properties["source"], "GFS 850 hPa");
        }
        // Off the grid, or calm: no point, and no direction invented.
        assert!(wind_barb_features(&wind, (-120.0, 10.0, -119.0, 11.0), 3, t, "x").is_empty());
        let calm: WindPair = (grid(0.0), grid(0.0));
        assert!(wind_barb_features(&calm, (-99.0, 35.0, -97.0, 37.0), 3, t, "x").is_empty());
        let json = wxdata::gis::to_geojson(&points);
        assert!(json.contains("\"from_deg\""), "{json}");
    }

    /// Live (network): the newest ECMWF run's 500 hPa temperature at F+12 minus the GFS's at the
    /// same run and lead, as the pane's difference fetches them. The two models agree to a few
    /// kelvin at 500 hPa a half day out. Writes `target/parity-review/m5.3/gfs-ecmwf-diff-live.txt`.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    #[ignore = "network: GFS and ECMWF inventories"]
    async fn gfs_minus_ecmwf_live() {
        use wxdata::global::{fetch_global_inventory_field, GlobalModel};
        let http = reqwest::Client::new();
        let ec = fetch_global_inventory_field(
            &http,
            GlobalModel::Ecmwf,
            None,
            12,
            "TMP",
            "500 mb",
            TimingKind::Instant,
        )
        .await
        .unwrap();
        let gfs = fetch_global_inventory_field(
            &http,
            GlobalModel::Gfs,
            Some(ec.run),
            12,
            "TMP",
            "500 mb",
            TimingKind::Instant,
        )
        .await
        .unwrap();
        assert_eq!(
            (gfs.run, gfs.valid()),
            (ec.run, ec.valid()),
            "same run and lead"
        );
        let d = crate::fielddiff::diff(&gfs.field, &ec.field).expect("overlapping grids");
        let mut v: Vec<f32> = d.values.iter().copied().filter(|x| x.is_finite()).collect();
        let mean = v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64;
        let mut abs: Vec<f32> = v.iter().map(|x| x.abs()).collect();
        abs.sort_by(f32::total_cmp);
        v.sort_by(f32::total_cmp);
        let p98 = abs[(abs.len() - 1) * 98 / 100];
        let report = format!(
            "GFS minus ECMWF, 500 hPa temperature, run {} F+12 (valid {})\ncells {} on a {}x{} lattice\nmean {:+.2} K, median |diff| {:.2} K, 98th percentile |diff| {:.2} K, range {:.2}..{:.2} K\nsymmetric display range (difference_range) {:.2} K\n",
            ec.run,
            ec.valid(),
            v.len(),
            d.nx,
            d.ny,
            mean,
            abs[abs.len() / 2],
            p98,
            v[0],
            v[v.len() - 1],
            difference_range(&d).unwrap()
        );
        print!("{report}");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m5.3");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gfs-ecmwf-diff-live.txt"), report).unwrap();
        assert!(v.len() > 100_000);
        assert!(mean.abs() < 2.0, "mean {mean}");
        assert!(p98 < 6.0, "98th percentile {p98}");
    }

    #[test]
    fn a_difference_is_its_own_product_on_a_symmetric_diverging_scale() {
        let base = SavedFieldPick {
            model: InventorySource::Gfs,
            var: "TMP".into(),
            level: "500 mb".into(),
            kind: TimingKind::Instant,
            vector: false,
            minus: None,
        };
        let diff = SavedFieldPick {
            minus: Some(InventorySource::Ecmwf),
            ..base.clone()
        };
        assert_ne!(base.pick().product_id(), diff.pick().product_id());
        assert!(
            diff.pick().product_id().ends_with(":minus:ECMWF"),
            "{}",
            diff.pick().product_id()
        );
        assert_eq!(base.pick().difference_label(), None);
        let label = diff.pick().difference_label().unwrap();
        assert!(label.contains("GFS") && label.contains("ECMWF"), "{label}");
        // Saved and reopened as the same difference; a pick saved before differences has none.
        let back: SavedFieldPick =
            serde_json::from_str(&serde_json::to_string(&diff).unwrap()).unwrap();
        assert_eq!(back, diff);
        assert!(!serde_json::to_string(&base).unwrap().contains("minus"));
        // The scale is symmetric about zero, white in the middle.
        let g = grid(vec![-3.0, -1.0, 0.5, 2.0]);
        let m = difference_range(&g).unwrap();
        assert!(m > 0.0);
        let t = difference_table(m);
        let mid = t.sample(0.0).unwrap();
        assert!(mid[0] > 230 && mid[1] > 230 && mid[2] > 230, "{mid:?}");
        let (lo, hi) = (t.sample(-m).unwrap(), t.sample(m).unwrap());
        assert!(lo[2] > lo[0], "below zero is blue: {lo:?}");
        assert!(hi[0] > hi[2], "above zero is red: {hi:?}");
    }

    #[test]
    fn a_wind_reads_the_direction_it_blows_from() {
        let from = |u, v| wind_from_deg(u, v).map(|d| d.round() as i64);
        assert_eq!(from(10.0, 0.0), Some(270), "blowing east: a westerly");
        assert_eq!(from(0.0, -10.0), Some(0), "blowing south: a northerly");
        assert_eq!(from(-5.0, 0.0), Some(90));
        assert_eq!(
            from(5.0, 5.0),
            Some(225),
            "toward the north-east: from the south-west"
        );
        assert_eq!(from(0.0, 0.0), None, "calm has no direction");
        assert_eq!(from(f32::NAN, 3.0), None);
    }

    #[test]
    fn a_pick_survives_saving_and_names_what_it_shows() {
        let saved = SavedFieldPick {
            model: InventorySource::Regional(Regional::HrrrPressure),
            var: "TMP".into(),
            level: "500 mb".into(),
            kind: TimingKind::Instant,
            vector: false,
            minus: None,
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
        // A scalar pick saves as it did before vectors existed.
        assert!(!json.contains("vector"), "{json}");
    }

    #[test]
    fn a_wind_pick_is_its_own_product() {
        let scalar = SavedFieldPick {
            model: InventorySource::Regional(Regional::Rap),
            var: "UGRD".into(),
            level: "500 mb".into(),
            kind: TimingKind::Instant,
            vector: false,
            minus: None,
        };
        let wind = SavedFieldPick {
            vector: true,
            ..scalar.clone()
        };
        let json = serde_json::to_string(&wind).unwrap();
        assert_eq!(serde_json::from_str::<SavedFieldPick>(&json).unwrap(), wind);
        assert_eq!(
            wind.pick().label(),
            "Wind \u{b7} 500 hPa \u{b7} instant (kt)"
        );
        assert_eq!(wind.pick().product_id(), "WIND:500 mb:instant");
        assert_eq!(wind.pick().unit(), "kt");
        assert_ne!(wind.pick().product_id(), scalar.pick().product_id());
        assert_ne!(wind.pick(), scalar.pick());
    }

    #[test]
    fn a_wind_reply_is_accepted_only_whole_and_only_by_a_wind_request() {
        use super::super::model_context::ModelRequest;
        let run = chrono::DateTime::from_timestamp(1_760_000_400 / 3600 * 3600, 0).unwrap();
        let valid = run + chrono::Duration::hours(3);
        let comp = |x: f32| {
            let mut f = grid(vec![x, x, f32::NEG_INFINITY, x]);
            f.time = valid;
            f
        };
        let (u, v) = (comp(3.0), comp(-4.0));
        let speed = wind_speed_kt(&u, &v).unwrap();
        assert!((speed.values[0] - 5.0 * MS_TO_KT).abs() < 1e-4);
        assert_eq!(
            speed.values[2],
            f32::NEG_INFINITY,
            "half a wind is no speed"
        );
        let wind = SavedFieldPick {
            model: InventorySource::Gfs,
            var: "UGRD".into(),
            level: "250 mb".into(),
            kind: TimingKind::Instant,
            vector: true,
            minus: None,
        }
        .pick();
        let stamped = super::super::field_state::model_field(
            wind.model.source_id(),
            &wind.product_id(),
            speed,
            Some(run),
            valid,
            true,
        )
        .unwrap();
        let msg = |uv: WindPair| OverlayMsg::VectorField(stamped.clone(), Box::new(uv));
        let asked = ModelRequest::Discovered(wind, 3, Some(run));
        assert!(asked.accepts_message(&msg((u.clone(), v.clone()))));
        // The scalar UGRD at that level is another product.
        let scalar = FieldPick {
            vector: false,
            ..wind
        };
        assert!(!ModelRequest::Discovered(scalar, 3, Some(run))
            .accepts_message(&msg((u.clone(), v.clone()))));
        // Components on another lattice than the speed are refused.
        let mut short = v.clone();
        short.values.pop();
        short.nx -= 1;
        assert!(!asked.accepts_message(&msg((u.clone(), short))));
    }

    #[test]
    fn components_follow_the_display_lattice_by_nearest_cell() {
        let mut f = grid((0..16).map(|i| i as f32 - 8.0).collect());
        f.nx = 4;
        f.ny = 4;
        let like = f.clone().decimated(2);
        let r = resample_like(f.clone(), &like);
        assert!(same_lattice(&r, &like));
        // Nearest cell keeps the sign a maximum pool would lose.
        assert_eq!(r.values, vec![-3.0, -1.0, 5.0, 7.0]);
        assert_eq!(resample_like(f.clone(), &f).values, f.values);
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
            vector: false,
            minus: None,
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
        // The same field from the ECMWF is a third source.
        let ecmwf = FieldPick {
            model: InventorySource::Ecmwf,
            ..gfs
        };
        assert!(!ModelRequest::Discovered(ecmwf, 120, Some(gfs_run)).accepts(&gfs_stamped.stamp));
    }

    #[test]
    fn a_global_lead_snaps_to_a_file_the_model_publishes() {
        let mut models = crate::model_pane::ModelControls {
            global_fcst_hour: 149,
            ..Default::default()
        };
        assert_eq!(InventorySource::Ecmwf.lead(&models), 144);
        assert_eq!(InventorySource::Gfs.lead(&models), 147);
        models.global_fcst_hour = 7;
        assert_eq!(InventorySource::Ecmwf.lead(&models), 6);
        assert_eq!(InventorySource::Gfs.lead(&models), 7);
        assert_eq!(InventorySource::Ecmwf.source_id(), "ECMWF");
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
