//! Column user-defined products on the map (ROADMAP_PARITY M3.3): a formula with a vertical/layer
//! function, evaluated over every tilt of the pane's own volume on the local-derived grid
//! (`wxdata::udp_column`) and drawn as `FieldLayer::UserColumn`.
//!
//! One build at a time, for the active pane, like the composite/VIL build beside it
//! (`rebuild.rs`) but on its own request lane. The key names everything the answer depends on —
//! the scan and accepted revision, the temporal policy, the product's definition and palette, and
//! the environmental heights the formula actually reads with their source — so a late worker for a
//! superseded selection is dropped, and a pane draws the shared texture only when its own
//! selection would have produced exactly that key.
use super::*;
use wxdata::level2::temporal::{TemporalCoverage, TemporalPolicy};
use wxdata::udp_column::{ColumnEnv, ColumnProduct, ColumnTilt};

/// The isotherm heights a formula reads, as bits, plus the source they came from. Heights the
/// formula does not read are left out, so a fresh sounding does not rebuild `max_vertical(REF)`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
struct EnvBits {
    antenna: Option<u32>,
    h0: Option<u32>,
    hm10: Option<u32>,
    hm20: Option<u32>,
    source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnKey {
    site: Option<String>,
    volume: String,
    revision: u64,
    scan: radar_products::ScanIdentity,
    acquisition: Option<crate::live_scan::AcquisitionSnapshot>,
    policy: TemporalPolicy,
    product: String,
    /// Formula, units, range and palette (with the palette registry's generation when it names a
    /// moment's table).
    definition: u64,
    env: EnvBits,
}

impl ColumnKey {
    fn pass_index(&self) -> Option<&wxdata::live_pass::PassAttributionIndex> {
        self.acquisition
            .as_ref()
            .and_then(crate::live_scan::AcquisitionSnapshot::pass_index)
    }
}

/// What one pane's selected column product evaluates as.
#[derive(Clone)]
pub(crate) struct ColumnSpec {
    pub name: String,
    pub units: String,
    pub expr: wxdata::udp::Expr,
    pub range: Option<(f32, f32)>,
    pub table: Option<crate::colormap::ColorTable>,
    pub env: ColumnEnv,
    /// Where the environmental heights came from, when the formula reads any.
    pub env_source: Option<String>,
}

/// An accepted build: what the resident `UserColumn` texture shows.
pub(crate) struct ColumnAccepted {
    key: ColumnKey,
    pub name: String,
    pub units: String,
    pub coverage: TemporalCoverage,
    pub product: ColumnProduct,
    /// The value range the colours span.
    pub range: (f32, f32),
    pub table: crate::colormap::ColorTable,
    pub env_source: Option<String>,
    pub volume_time: chrono::DateTime<chrono::Utc>,
}

pub(crate) struct ColumnDelivery {
    key: ColumnKey,
    name: String,
    units: String,
    range: Option<(f32, f32)>,
    table: Option<crate::colormap::ColorTable>,
    env_source: Option<String>,
    volume_time: chrono::DateTime<chrono::Utc>,
    result: Result<(TemporalCoverage, ColumnProduct), String>,
}

impl ColumnDelivery {
    pub(super) fn error(&self) -> Option<&str> {
        self.result.as_ref().err().map(String::as_str)
    }

    /// Newest source radial time the build used, for source health.
    pub(super) fn acquisition_end(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.result
            .as_ref()
            .ok()?
            .0
            .acquisition_range_ms()
            .and_then(|(_, end)| chrono::DateTime::from_timestamp_millis(end))
    }
}

/// Why the selected column product is not on the map, for the legend and the product window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ColumnStatus {
    Building,
    Unavailable(String),
}

fn env_bits(spec: &ColumnSpec) -> EnvBits {
    use wxdata::udp::Input;
    let reads = spec.expr.inputs();
    let pick = |input: Input, v: Option<f32>| {
        reads
            .contains(&input)
            .then_some(v)
            .flatten()
            .map(f32::to_bits)
    };
    let l = spec.env.levels;
    let bits = EnvBits {
        antenna: pick(Input::BeamAltitudeM, spec.env.antenna_altitude_m),
        h0: pick(Input::FreezingLevelM, l.h0_m),
        hm10: pick(Input::Minus10cHeightM, l.hm10_m),
        hm20: pick(Input::Minus20cHeightM, l.hm20_m),
        source: None,
    };
    let reads_env = bits.h0.is_some() || bits.hm10.is_some() || bits.hm20.is_some();
    EnvBits {
        source: spec.env_source.clone().filter(|_| reads_env),
        ..bits
    }
}

/// The identity of `spec` drawn from `view`'s volume: everything the answer depends on.
fn key_for(
    view: &MapView,
    settings: &Settings,
    palettes_gen: u64,
    spec: &ColumnSpec,
) -> Option<ColumnKey> {
    let vol = view.volume.as_ref()?;
    Some(ColumnKey {
        site: view.site.clone(),
        volume: vol.name.clone(),
        revision: vol.revision(),
        scan: radar_products::ScanIdentity::new(&vol.scan),
        acquisition: vol.acquisition_for(view.site.as_deref()).cloned(),
        policy: radar_products::policy(view, settings),
        product: spec.name.clone(),
        definition: definition_hash(settings, palettes_gen, &spec.name),
        env: env_bits(spec),
    })
}

/// The saved definition named `name` — formula, units, range, palette (with the palette
/// registry's generation and theme when it names a moment's table) — as one number.
fn definition_hash(settings: &Settings, palettes_gen: u64, name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(def) = settings.udp_products.iter().find(|p| p.name == name) {
        def.expression.hash(&mut h);
        def.units.hash(&mut h);
        def.palette.hash(&mut h);
        def.range
            .map(|(a, b)| (a.to_bits(), b.to_bits()))
            .hash(&mut h);
        if def.palette.is_some() {
            palettes_gen.hash(&mut h);
            // The high-contrast theme swaps a default table for its alternative.
            crate::theme::is_high_contrast(settings.theme).hash(&mut h);
        }
    }
    h.finish()
}

/// Everything about `spec` but the volume: its definition and the environment it reads, as one
/// number — what a trail of this product over many volumes is keyed by.
pub(super) fn product_identity(settings: &Settings, palettes_gen: u64, spec: &ColumnSpec) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    definition_hash(settings, palettes_gen, &spec.name).hash(&mut h);
    spec.name.hash(&mut h);
    env_bits(spec).hash(&mut h);
    h.finish()
}

/// The tilts of a decoded scan (not a pane's `Volume`) for a column formula, as
/// [`assemble_tilts`] takes them: every distinct tilt, the moments it reads, velocity dealiased.
pub(super) fn scan_tilts(scan: &wxdata::level2::Scan, expr: &wxdata::udp::Expr) -> Vec<ColumnTilt> {
    let moments = wxdata::udp_volume::MOMENTS;
    let reads = expr.inputs();
    let mut wanted: Vec<usize> = (0..moments.len())
        .filter(|&i| reads.contains(&wxdata::udp_volume::moment_input(moments[i])))
        .collect();
    if wanted.is_empty() {
        wanted.push(0);
    }
    let mut tilts: Vec<ColumnTilt> = (0..wxdata::level2::elevation_angles(scan).len())
        .map(|t| {
            std::array::from_fn(|i| {
                wanted
                    .contains(&i)
                    .then(|| {
                        wxdata::level2::bin_scan_opts(
                            scan,
                            moments[i],
                            t,
                            moments[i] == Moment::Velocity,
                        )
                        .ok()
                    })
                    .flatten()
            })
        })
        .collect();
    tilts.retain(|t| t.iter().any(Option::is_some));
    tilts
}

/// The product a pane's volume shows, as the worker builds it (continuous policy), for tests that
/// compare it with another path.
#[cfg(test)]
pub(super) fn single_volume_product(
    vol: &mut Volume,
    spec: &ColumnSpec,
    time: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<ColumnProduct> {
    let tilts = assemble_tilts(vol, &spec.expr);
    Ok(build(
        tilts,
        TemporalPolicy::Continuous,
        None,
        &spec.expr,
        &spec.env,
        time,
    )?
    .1)
}

/// The worker's evaluation of one scan under the continuous policy (a loop frame is a whole
/// volume), stamped with `time`.
pub(super) fn evaluate_scan(
    scan: &wxdata::level2::Scan,
    expr: &wxdata::udp::Expr,
    env: &ColumnEnv,
    time: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<ColumnProduct> {
    let tilts = scan_tilts(scan, expr);
    Ok(build(tilts, TemporalPolicy::Continuous, None, expr, env, time)?.1)
}

/// Every distinct tilt of `vol` with the moments `expr` reads (velocity dealiased, as every
/// product reads it), binned through the volume's cache. A geometry-only formula takes each
/// tilt's reflectivity for its beams. Tilts with none of them are left out.
fn assemble_tilts(vol: &mut Volume, expr: &wxdata::udp::Expr) -> Vec<ColumnTilt> {
    let moments = wxdata::udp_volume::MOMENTS;
    let reads = expr.inputs();
    let mut wanted: Vec<usize> = (0..moments.len())
        .filter(|&i| reads.contains(&wxdata::udp_volume::moment_input(moments[i])))
        .collect();
    if wanted.is_empty() {
        wanted.push(0);
    }
    let mut tilts: Vec<ColumnTilt> = (0..vol.elevations.len())
        .map(|t| {
            std::array::from_fn(|i| {
                wanted
                    .contains(&i)
                    .then(|| {
                        vol.binned(moments[i], t, moments[i] == Moment::Velocity)
                            .ok()
                            .cloned()
                    })
                    .flatten()
            })
        })
        .collect();
    tilts.retain(|t| t.iter().any(Option::is_some));
    tilts
}

/// The worker's half: apply the temporal policy over every contributing sweep at once, then
/// evaluate. Pure, so a test can run exactly what the worker runs.
fn build(
    mut tilts: Vec<ColumnTilt>,
    policy: TemporalPolicy,
    passes: Option<&wxdata::live_pass::PassAttributionIndex>,
    expr: &wxdata::udp::Expr,
    env: &ColumnEnv,
    time: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(TemporalCoverage, ColumnProduct)> {
    let mut flat: Vec<wxdata::level2::BinnedSweep> = Vec::new();
    let mut slots = Vec::new();
    for (t, tilt) in tilts.iter_mut().enumerate() {
        for (i, s) in tilt.iter_mut().enumerate() {
            if let Some(s) = s.take() {
                slots.push((t, i));
                flat.push(s);
            }
        }
    }
    let coverage = wxdata::level2::temporal::prepare_with_passes(&mut flat, policy, passes)?;
    for ((t, i), s) in slots.into_iter().zip(flat) {
        tilts[t][i] = Some(s);
    }
    let product = wxdata::udp_column::evaluate_grid(expr, &tilts, env, time)?;
    Ok((coverage, product))
}

/// Rows that are not from the current pass, said plainly: kept under the continuous policy, masked
/// under strict-current. Empty when every row is from one pass.
fn coverage_note(c: &TemporalCoverage) -> String {
    let mut out = String::new();
    let older = c.retained_older_rows();
    if older > 0 {
        out.push_str(&format!(" · {older} older-pass rows kept"));
    }
    let masked = c.excluded_rows();
    if masked > 0 {
        out.push_str(&format!(" · {masked} rows masked (strict)"));
    }
    let untimed = c.unknown_time_rows();
    if untimed > 0 {
        out.push_str(&format!(" · {untimed} rows untimed"));
    }
    out
}

/// The colour range: the product's own, else its table's span, else what it came out in.
fn colour_range(spec_range: Option<(f32, f32)>, product: &ColumnProduct) -> Option<(f32, f32)> {
    let values: Vec<Option<f32>> = product
        .field
        .values
        .iter()
        .map(|v| v.is_finite().then_some(*v))
        .collect();
    wxdata::udp_volume::auto_range(values.iter(), spec_range)
}

/// The GPU upload for an accepted column grid: values quantized over `range` into 2..=255, the
/// product's table baked over the same range. NaN stays transparent.
pub(crate) fn column_upload(
    f: &wxdata::mrms::MrmsField,
    table: &crate::colormap::ColorTable,
    range: (f32, f32),
) -> crate::render::MrmsUpload {
    let (lo, hi) = range;
    let span = (hi - lo).max(f32::EPSILON);
    super::field_index_upload(
        f,
        |v| {
            if v.is_finite() {
                (2.0 + ((v - lo) / span).clamp(0.0, 1.0) * 253.0).round() as u8
            } else {
                0
            }
        },
        crate::colormap::bake_lut(table, range, None).to_vec(),
    )
}

impl HookEchoApp {
    /// Pane `idx`'s selected column product, compiled with the environment matched to its own site
    /// and time. `None` with no selection; `Err` when it cannot be a column product at all.
    pub(crate) fn column_spec(&self, idx: usize) -> Option<Result<ColumnSpec, String>> {
        let v = &self.views[idx];
        let name = v.column_product.as_ref()?;
        let Some(def) = self.settings.udp_products.iter().find(|p| &p.name == name) else {
            return Some(Err(format!("no saved product named “{name}”")));
        };
        let expr = match def.compile() {
            Ok(e) => e,
            Err(e) => return Some(Err(e.to_string())),
        };
        if !expr.uses_column() {
            return Some(Err(
                "a gate formula: show it on the map as a tilt product instead".into(),
            ));
        }
        let antenna_altitude_m = v
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| s.elevation_meters as f32 + wxdata::towers::tower_m(s.id) as f32);
        let env_levels = self.env_levels_for(idx);
        let table = def
            .palette
            .as_deref()
            .and_then(Moment::from_code)
            .map(|m| crate::colormap::effective_table(&self.palettes, m, self.settings.theme));
        let table_span = table.as_ref().and_then(|t| {
            let (lo, hi) = (t.stops.first()?.value, t.stops.last()?.value);
            (hi > lo).then_some((lo, hi))
        });
        Some(Ok(ColumnSpec {
            name: def.name.clone(),
            units: def.units.clone(),
            expr,
            range: def.range.or(table_span),
            table,
            env: ColumnEnv {
                antenna_altitude_m,
                levels: env_levels
                    .map(env_levels::EnvLevels::column_levels)
                    .unwrap_or_default(),
            },
            env_source: env_levels.map(|l| l.source.clone()),
        }))
    }

    fn column_key_for(&self, idx: usize, spec: &ColumnSpec) -> Option<ColumnKey> {
        key_for(&self.views[idx], &self.settings, self.palettes.gen, spec)
    }

    /// The key pane `idx` would draw, when its selection is complete enough to have one.
    fn column_current_key(&self, idx: usize) -> Option<ColumnKey> {
        let spec = self.column_spec(idx)?.ok()?;
        self.column_key_for(idx, &spec)
    }

    /// Whether pane `idx` may draw the resident `UserColumn` texture: only when its own selection
    /// is exactly the accepted build's.
    pub(crate) fn column_field_ready(&self, idx: usize) -> bool {
        self.column_accepted
            .as_ref()
            .is_some_and(|a| Some(&a.key) == self.column_current_key(idx).as_ref())
    }

    /// The accepted build pane `idx` is drawing, for its legend, probe and inspector.
    pub(crate) fn column_shown(&self, idx: usize) -> Option<&ColumnAccepted> {
        self.column_field_ready(idx)
            .then_some(self.column_accepted.as_deref())
            .flatten()
    }

    /// Why pane `idx`'s column product is not drawn, if it is selected and not drawn.
    pub(crate) fn column_status(&self, idx: usize) -> Option<ColumnStatus> {
        if !self.views[idx]
            .fields_on
            .contains(&crate::render::FieldLayer::UserColumn)
        {
            return None;
        }
        let Some(spec) = self.column_spec(idx) else {
            return Some(ColumnStatus::Unavailable(
                "choose a column product in User-defined products".into(),
            ));
        };
        let spec = match spec {
            Ok(s) => s,
            Err(e) => return Some(ColumnStatus::Unavailable(e)),
        };
        if self.column_field_ready(idx) {
            return None;
        }
        let missing = spec.env.missing_for(&spec.expr);
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|i| i.name()).collect();
            let live = self.views[idx].timeline.following;
            return Some(ColumnStatus::Unavailable(format!(
                "waiting for {} at this radar {}; nothing is substituted",
                names.join(", "),
                if live {
                    "from the live analysis"
                } else {
                    "from that day's sounding"
                }
            )));
        }
        let key = self.column_key_for(idx, &spec);
        match &self.column_failed {
            Some((k, e)) if key.as_ref() == Some(k) => Some(ColumnStatus::Unavailable(e.clone())),
            _ => Some(ColumnStatus::Building),
        }
    }

    /// Start a build when the active pane's column product, volume, policy or environment moved.
    pub(crate) fn recompute_column_product(&mut self, ctx: &egui::Context) {
        let idx = self.active;
        // An evicted texture takes the accepted build with it: nothing resident to draw.
        if self
            .fields
            .get(&crate::render::FieldLayer::UserColumn)
            .is_none_or(|s| s.grid.is_none())
        {
            self.column_accepted = None;
        }
        if !self.views[idx]
            .fields_on
            .contains(&crate::render::FieldLayer::UserColumn)
        {
            self.column_requested = None;
            return;
        }
        let Some(Ok(spec)) = self.column_spec(idx) else {
            self.column_requested = None;
            return;
        };
        // A formula reading an isotherm needs that site's and time's reading; ask for it and wait.
        let reads = spec.expr.inputs();
        use wxdata::udp::Input;
        if [
            Input::FreezingLevelM,
            Input::Minus10cHeightM,
            Input::Minus20cHeightM,
        ]
        .iter()
        .any(|i| reads.contains(i))
            && self.env_levels_for(idx).is_none()
        {
            self.fetch_freezing_levels(ctx, idx);
        }
        let Some(key) = self.column_key_for(idx, &spec) else {
            self.column_requested = None;
            return;
        };
        if self.column_requested.as_ref() == Some(&key)
            || self.column_accepted.as_ref().is_some_and(|a| a.key == key)
        {
            return;
        }
        if !spec.env.missing_for(&spec.expr).is_empty() {
            // Not an error to remember: the build starts the moment the reading lands.
            self.column_requested = None;
            return;
        }
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return;
        };
        let volume_time = vol.time;
        // Binning is cached on the volume; the column evaluation is the expensive half and runs
        // off-thread.
        let tilts = assemble_tilts(vol, &spec.expr);
        self.column_requested = Some(key.clone());
        let tx = self.overlay_tx.clone();
        let lane = RequestLane::Feed(FeedSource::UserColumnProduct);
        let generation = self.acquisition.start(lane.clone());
        let ctx = ctx.clone();
        let ColumnSpec {
            name,
            units,
            expr,
            range,
            table,
            env,
            env_source,
        } = spec;
        self.spawner.spawn_blocking(move || {
            let result = build(
                tilts,
                key.policy,
                key.pass_index(),
                &expr,
                &env,
                volume_time,
            )
            .map_err(|error| error.to_string());
            let _ = tx.send(OverlayDelivery::Fetched {
                model_request: None,
                mrms_context: None,
                lane,
                generation,
                result: Ok(OverlayMsg::ColumnProduct(Box::new(ColumnDelivery {
                    key,
                    name,
                    units,
                    range,
                    table,
                    env_source,
                    volume_time,
                    result,
                }))),
            });
            ctx.request_repaint();
        });
    }

    /// The key card for pane `idx`'s column product: its table over the range it is drawn in,
    /// with the environment's source under it, or one line saying why nothing is drawn yet.
    pub(crate) fn paint_column_key(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        idx: usize,
        y: f32,
    ) -> f32 {
        if let Some(a) = self.column_shown(idx) {
            let tz = self.active_tz();
            let span = a
                .coverage
                .acquisition_range_ms()
                .and_then(|(s, e)| {
                    Some((
                        chrono::DateTime::from_timestamp_millis(s)?,
                        chrono::DateTime::from_timestamp_millis(e)?,
                    ))
                })
                .map_or_else(
                    // No source radial clocks: say the volume's own time, not a made-up span.
                    || {
                        format!(
                            "volume {}",
                            crate::timefmt::fmt_clock(a.volume_time, tz, false)
                        )
                    },
                    |(s, e)| {
                        format!(
                            "{}–{}",
                            crate::timefmt::fmt_clock(s, tz, false),
                            crate::timefmt::fmt_clock(e, tz, false)
                        )
                    },
                );
            let mut note = format!("{} tilts · {span}", a.product.tilts);
            note.push_str(&coverage_note(&a.coverage));
            if let Some(src) = &a.env_source {
                note.push_str(&format!(" · env {src}"));
            }
            return crate::ui::legend::draw_table_card(
                painter,
                prect,
                &a.name,
                &a.units,
                &a.table,
                a.range,
                Some(&note),
                y,
            );
        }
        let text = match self.column_status(idx) {
            Some(ColumnStatus::Unavailable(why)) => format!("Column product: {why}"),
            Some(ColumnStatus::Building) => "Column product: building from this volume…".into(),
            None => return 0.0,
        };
        crate::ui::legend::draw_status_card(painter, prect, &text, y)
    }

    /// Whether a delivery still answers what the active pane asks for.
    pub(super) fn column_delivery_current(&self, delivery: &ColumnDelivery) -> bool {
        self.column_requested.as_ref() == Some(&delivery.key)
            && self.column_current_key(self.active).as_ref() == Some(&delivery.key)
    }

    pub(super) fn accept_column_product(&mut self, delivery: ColumnDelivery) {
        if !self.column_delivery_current(&delivery) {
            return;
        }
        self.column_requested = None;
        let ColumnDelivery {
            key,
            name,
            units,
            range,
            table,
            env_source,
            volume_time,
            result,
        } = delivery;
        let (coverage, product) = match result {
            Ok(ok) => ok,
            Err(e) => {
                self.column_failed = Some((key, e));
                return;
            }
        };
        let Some(range) = colour_range(range, &product) else {
            self.column_failed = Some((
                key,
                "the product has no value anywhere in this volume".into(),
            ));
            return;
        };
        let table = table.unwrap_or_else(|| crate::colormap::ramp_table(range.0, range.1));
        let field = product.field.clone();
        let upload = column_upload(&field, &table, range);
        if let Some(state) = self.fields.get_mut(&crate::render::FieldLayer::UserColumn) {
            state.stage(field, None, upload);
            state.radar = None;
        }
        self.column_failed = None;
        self.column_accepted = Some(Arc::new(ColumnAccepted {
            key,
            name,
            units,
            coverage,
            product,
            range,
            table,
            env_source,
            volume_time,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(values: Vec<f32>) -> wxdata::mrms::MrmsField {
        wxdata::mrms::MrmsField {
            nx: values.len(),
            ny: 1,
            values,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 36.0,
            lat_south: 35.0,
            time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    fn fixture_view(scan: Arc<wxdata::level2::Scan>) -> MapView {
        let time = chrono::DateTime::from_timestamp(1_639_193_029, 0).unwrap();
        let mut view = MapView::new(
            Some("KPAH".into()),
            crate::render::mercator::Camera::at_lonlat(-88.0, 37.0, 8.0),
        );
        view.timeline.following = true;
        view.volume = Some(Volume::from_live(
            scan,
            "KPAH20211211_032349_V06".into(),
            time,
        ));
        view
    }

    fn mayfield() -> Arc<wxdata::level2::Scan> {
        Arc::new(
            wxdata::level2::decode_volume(
                include_bytes!("../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                    .to_vec(),
            )
            .unwrap(),
        )
    }

    fn spec_of(src: &str) -> ColumnSpec {
        ColumnSpec {
            name: "Core CC".into(),
            units: String::new(),
            expr: wxdata::udp::parse(src).unwrap(),
            range: None,
            table: None,
            env: ColumnEnv::default(),
            env_source: None,
        }
    }

    fn settings_with(src: &str) -> Settings {
        Settings {
            udp_products: vec![wxdata::udp::ProductDef {
                id: String::new(),
                name: "Core CC".into(),
                units: String::new(),
                expression: src.into(),
                range: None,
                palette: None,
            }],
            ..Settings::default()
        }
    }

    #[test]
    fn the_key_changes_with_scan_policy_definition_and_site_and_nothing_else() {
        let scan = mayfield();
        let src = "min_vertical(CC, REF >= 40)";
        let mut settings = settings_with(src);
        let view = fixture_view(Arc::clone(&scan));
        let key = key_for(&view, &settings, 0, &spec_of(src)).unwrap();
        assert_eq!(
            Some(&key),
            key_for(&view, &settings, 0, &spec_of(src)).as_ref()
        );
        // The palette registry moving does not touch a ramp-coloured product.
        assert_eq!(
            Some(&key),
            key_for(&view, &settings, 9, &spec_of(src)).as_ref()
        );

        // The same bytes decoded again are another scan: a worker for the old one cannot land.
        let again = fixture_view(mayfield());
        assert_ne!(
            Some(&key),
            key_for(&again, &settings, 0, &spec_of(src)).as_ref()
        );

        // Another radar's pane never shares a build.
        let mut other = fixture_view(Arc::clone(&scan));
        other.site = Some("KLZK".into());
        assert_ne!(
            Some(&key),
            key_for(&other, &settings, 0, &spec_of(src)).as_ref()
        );

        // Editing the saved formula is a different product.
        settings.udp_products[0].expression = "min_vertical(CC, REF >= 45)".into();
        assert_ne!(
            Some(&key),
            key_for(&view, &settings, 0, &spec_of(src)).as_ref()
        );
        settings.udp_products[0].expression = src.into();

        // A colour-table product follows the registry and the theme.
        settings.udp_products[0].palette = Some("CC".into());
        let k0 = key_for(&view, &settings, 0, &spec_of(src));
        assert_ne!(k0, key_for(&view, &settings, 1, &spec_of(src)));

        // Strict-current on a partial live volume is a different answer from continuous.
        settings.udp_products[0].palette = None;
        settings.live_sweep_mode = crate::settings::LiveSweepMode::StrictCurrentSweep;
        let strict = key_for(&view, &settings, 0, &spec_of(src)).unwrap();
        if view.volume.as_ref().unwrap().is_live_partial() {
            assert_ne!(strict, key, "policy is part of the identity");
        }
        assert_eq!(strict.policy, radar_products::policy(&view, &settings));
    }

    #[test]
    fn a_real_partial_volume_builds_the_same_product_twice_at_its_own_clock() {
        let scan = mayfield();
        let mut view = fixture_view(scan);
        let spec = spec_of("max_vertical(REF)");
        let vol = view.volume.as_mut().unwrap();
        let time = vol.time;
        let run = |vol: &mut Volume| {
            build(
                assemble_tilts(vol, &spec.expr),
                TemporalPolicy::Continuous,
                None,
                &spec.expr,
                &spec.env,
                time,
            )
            .unwrap()
        };
        let (coverage, a) = run(vol);
        let (_, b) = run(vol);
        assert!(a.cells_with_value > 100, "{a:?}");
        assert_eq!(a.field.time, time, "the volume's clock, never now");
        assert!(a.field.values.iter().map(|v| v.to_bits()).eq(b
            .field
            .values
            .iter()
            .map(|v| v.to_bits())));
        assert_eq!(a.levels_sampled, b.levels_sampled);
        // The partial file holds only its first records: unobserved azimuths stay empty rather
        // than being filled, and the coverage says what was used.
        assert!(a.field.values.iter().any(|v| v.is_nan()));
        assert!(coverage.acquisition_range_ms().is_some());
        // And it is the local composite of the same tilts.
        let refl: Vec<_> = assemble_tilts(vol, &spec.expr)
            .into_iter()
            .filter_map(|t| t[0].clone())
            .collect();
        let d = wxdata::derived::derive(
            &refl,
            &wxdata::derived::DerivedOpts {
                time,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(a.field.values.iter().map(|v| v.to_bits()).eq(d
            .composite
            .values
            .iter()
            .map(|v| v.to_bits())));
    }

    #[test]
    fn the_upload_quantizes_over_the_colour_range_and_keeps_missing_clear() {
        let f = field(vec![f32::NAN, 0.0, 50.0, 100.0, 1e6]);
        let table = crate::colormap::ramp_table(0.0, 100.0);
        let up = column_upload(&f, &table, (0.0, 100.0));
        assert_eq!(up.data, [0, 2, 129, 255, 255]);
        // Index 0 is transparent in the baked table.
        assert_eq!(up.lut[3], 0);
    }

    #[test]
    fn the_key_ignores_isotherms_the_formula_does_not_read() {
        let spec = |src: &str, h0: f32, hm10: Option<f32>| ColumnSpec {
            name: "p".into(),
            units: String::new(),
            expr: wxdata::udp::parse(src).unwrap(),
            range: None,
            table: None,
            env: ColumnEnv {
                antenna_altitude_m: Some(370.0),
                levels: wxdata::udp_column::Levels {
                    h0_m: Some(h0),
                    hm10_m: hm10,
                    hm20_m: Some(7000.0),
                },
            },
            env_source: Some("HRRR analysis 06 00Z".into()),
        };
        let plain = |h0| env_bits(&spec("max_vertical(REF)", h0, None));
        assert_eq!(
            plain(3000.0),
            plain(4000.0),
            "no rebuild for an unread level"
        );
        assert_eq!(plain(3000.0).source, None);
        let hot = |h0| {
            env_bits(&spec(
                "max_vertical(ZDR, BEAM_ALTITUDE_M > FREEZING_LEVEL_M)",
                h0,
                None,
            ))
        };
        assert_ne!(
            hot(3000.0),
            hot(4000.0),
            "a read level is part of the identity"
        );
        assert!(hot(3000.0).source.is_some(), "and so is where it came from");
        let m10 = |hm10| {
            env_bits(&spec(
                "max_vertical(REF, BEAM_ALTITUDE_M > MINUS10C_HEIGHT_M)",
                3000.0,
                hm10,
            ))
        };
        assert_ne!(m10(Some(5000.0)), m10(None));
    }

    #[test]
    fn an_explicit_range_wins_and_an_empty_product_has_none() {
        let p = |values: Vec<f32>| wxdata::udp_column::ColumnProduct {
            levels_sampled: vec![0; values.len()],
            field: field(values),
            tilts: 1,
            cells_with_value: 0,
        };
        assert_eq!(
            colour_range(Some((0.0, 10.0)), &p(vec![1.0, 50.0])),
            Some((0.0, 10.0))
        );
        assert_eq!(colour_range(None, &p(vec![f32::NAN, f32::NAN])), None);
        let (lo, hi) = colour_range(None, &p((0..100).map(|i| i as f32).collect())).unwrap();
        assert!(lo < 5.0 && hi > 95.0, "{lo}..{hi}");
    }
}
