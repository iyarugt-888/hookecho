//! The flood-gauge dashboard: every river gauge the map's gauge layer has fetched, in one place.
//!
//! Top to bottom: how many gauges are at each flood category (a bar that filters the table when
//! a segment is clicked), counts of what is flooding, forecast to flood and rising, the worst
//! gauges' change over the past week on one chart, then a table of every gauge — category,
//! stage, forecast, a sparkline of the week and the change over 24 hours — and below it the
//! selected gauge's full card (the one a map click opens: hydrograph out to 30 days, flood
//! stages, crest history, impacts), drawn in place by [`Cards::show_focus`].
//!
//! The list is the gauge layer's own bounding-box fetch, so the dashboard costs nothing extra to
//! list. Sparklines need each gauge's hydrograph, and NWPS allows ten requests in five minutes,
//! so they are fetched one gauge a minute while the dashboard is on screen: gauges in or forecast
//! to reach flooding first, then the rest in table order, each kept for 20 minutes.
//!
//! The workstation docks it as a tool window (`app::chrome::dock::gauges`); the other layouts show
//! it as a plain floating window.

use crate::ui::gauge_card::{self, cat_color, cat_label, Cards};
use crate::ui::workstation as ws;
use chrono::{DateTime, Duration, Utc};
use egui::{Color32, FontId, Rect, Sense, Stroke};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use wxdata::clock::Instant;
use wxdata::river::{FloodCat, Gauge, Reading, Trend};
use wxdata::tz::Tz;

/// One sparkline fetch at most this often (the service's budget is ten requests in five minutes,
/// shared with the list and any open cards).
const SPARK_GAP_SECS: u64 = 60;
/// A sparkline is refetched once it is this old.
const SPARK_MAX_AGE_SECS: u64 = 20 * 60;
/// After the service says it is rate limiting, sparklines wait this long.
const SPARK_BACKOFF_SECS: u64 = 5 * 60;
/// How far back a sparkline (and the week chart) reaches.
const SPARK_HOURS: i64 = 7 * 24;
/// Sparklines are fetched for at most this many gauges from the top of the table.
const SPARK_REACH: usize = 40;
/// Lines on the week chart.
const CHART_LINES: usize = 5;

const ROW_H: f32 = 22.0;

/// Which gauges the table lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    /// At action stage or worse now.
    Flooding,
    /// Forecast to reach action stage or worse.
    Forecast,
    /// Rising over the last three hours (needs its sparkline).
    Rising,
    /// One category, picked on the bar.
    Category(FloodCat),
}

/// How the table is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Worst category first, then worst forecast, then highest stage.
    #[default]
    Severity,
    Name,
    /// Highest stage first.
    Stage,
    /// Biggest rise over 24 hours first.
    Change,
}

impl Sort {
    const ALL: [Sort; 4] = [Sort::Severity, Sort::Name, Sort::Stage, Sort::Change];
    fn label(self) -> &'static str {
        match self {
            Sort::Severity => "Severity",
            Sort::Name => "Name",
            Sort::Stage => "Stage",
            Sort::Change => "24 h",
        }
    }
}

/// A gauge's recent observed record, for its sparkline and the change columns.
struct Spark {
    readings: Vec<Reading>,
    fetched: Instant,
}

type Landed = (String, Result<Vec<Reading>, String>);

/// Everything the dashboard remembers between frames.
pub struct Dashboard {
    pub filter: Filter,
    pub sort: Sort,
    pub query: String,
    /// The floating window's open state, in the layouts that are not the workstation.
    pub window_open: bool,
    sparks: HashMap<String, Spark>,
    last_spark: Option<Instant>,
    in_flight: Option<String>,
    rate_limited: Option<Instant>,
    tx: Sender<Landed>,
    rx: Receiver<Landed>,
}

impl Default for Dashboard {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            filter: Filter::All,
            sort: Sort::Severity,
            query: String::new(),
            window_open: false,
            sparks: HashMap::new(),
            last_spark: None,
            in_flight: None,
            rate_limited: None,
            tx,
            rx,
        }
    }
}

/// Action stage or worse.
pub fn flooding(c: FloodCat) -> bool {
    c.severity() <= FloodCat::Action.severity()
}

/// Forecast to reach a worse category than it is in now, and one that floods.
pub fn worsening(g: &Gauge) -> bool {
    flooding(g.forecast_cat) && g.forecast_cat.severity() < g.cat.severity()
}

/// How many gauges are at each category, worst first (indexed by [`FloodCat::severity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub counts: [usize; 6],
    /// Forecast to be at action stage or worse.
    pub forecast_flooding: usize,
    /// Forecast to reach a worse category than now.
    pub worsening: usize,
}

impl Summary {
    pub fn total(&self) -> usize {
        self.counts.iter().sum()
    }
    /// Minor flooding or worse.
    pub fn in_flood(&self) -> usize {
        self.counts[..3].iter().sum()
    }
}

/// The categories in [`Summary::counts`] order.
pub const CATS: [FloodCat; 6] = [
    FloodCat::Major,
    FloodCat::Moderate,
    FloodCat::Minor,
    FloodCat::Action,
    FloodCat::NoFlooding,
    FloodCat::Unknown,
];

pub fn summarize(gauges: &[Gauge]) -> Summary {
    let mut s = Summary::default();
    for g in gauges {
        s.counts[g.cat.severity() as usize] += 1;
        s.forecast_flooding += usize::from(flooding(g.forecast_cat));
        s.worsening += usize::from(worsening(g));
    }
    s
}

/// The newest stage against the one at least `hours` before it. `None` when the record does not
/// reach back that far.
pub fn change_over(readings: &[Reading], hours: i64) -> Option<f64> {
    let last = readings.iter().rev().find(|r| r.stage_ft.is_some())?;
    let before = last.time - Duration::hours(hours);
    let then = readings
        .iter()
        .rev()
        .find(|r| r.stage_ft.is_some() && r.time <= before)?;
    Some(last.stage_ft? - then.stage_ft?)
}

/// What a gauge's sparkline says: the change over a day and which way it is going now.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RowStats {
    pub change_24h: Option<f64>,
    pub trend: Option<Trend>,
    /// The lowest and highest stage over the sparkline's week.
    pub range: Option<(f64, f64)>,
}

impl RowStats {
    fn of(readings: &[Reading], now: DateTime<Utc>) -> RowStats {
        let week: Vec<f64> = readings
            .iter()
            .filter(|r| r.time >= now - Duration::hours(SPARK_HOURS))
            .filter_map(|r| r.stage_ft)
            .collect();
        let range = (!week.is_empty()).then(|| {
            week.iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                    (a.min(*v), b.max(*v))
                })
        });
        RowStats {
            change_24h: change_over(readings, 24),
            trend: wxdata::river::trend(readings, 3.0),
            range,
        }
    }

    fn rising(&self) -> bool {
        matches!(self.trend, Some(Trend::Rising(_)))
    }
}

/// The table's rows: indexes into `gauges` passing the filter and the search, in the sort's order.
pub fn rows(
    gauges: &[Gauge],
    stats: &HashMap<&str, RowStats>,
    filter: Filter,
    sort: Sort,
    query: &str,
) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let stat = |g: &Gauge| stats.get(g.lid.as_str()).copied().unwrap_or_default();
    let mut out: Vec<usize> = gauges
        .iter()
        .enumerate()
        .filter(|(_, g)| match filter {
            Filter::All => true,
            Filter::Flooding => flooding(g.cat),
            Filter::Forecast => flooding(g.forecast_cat),
            Filter::Rising => stat(g).rising(),
            Filter::Category(c) => g.cat == c,
        })
        .filter(|(_, g)| {
            let hay = format!("{} {}", g.lid, g.name).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect();
    // Missing numbers sort last whichever way the column runs.
    let desc = |v: Option<f64>| v.map_or(f64::INFINITY, |v| -v);
    match sort {
        Sort::Severity => out.sort_by(|&a, &b| {
            let (ga, gb) = (&gauges[a], &gauges[b]);
            (ga.cat.severity(), ga.forecast_cat.severity())
                .cmp(&(gb.cat.severity(), gb.forecast_cat.severity()))
                .then(desc(ga.stage_ft).total_cmp(&desc(gb.stage_ft)))
        }),
        Sort::Name => out.sort_by(|&a, &b| gauges[a].name.cmp(&gauges[b].name)),
        Sort::Stage => {
            out.sort_by(|&a, &b| desc(gauges[a].stage_ft).total_cmp(&desc(gauges[b].stage_ft)))
        }
        Sort::Change => out.sort_by(|&a, &b| {
            desc(stat(&gauges[a]).change_24h).total_cmp(&desc(stat(&gauges[b]).change_24h))
        }),
    }
    out
}

/// Which gauge's sparkline to fetch next: in or forecast to reach flooding first, then the rest,
/// each in table order, among the first [`SPARK_REACH`]; skipping any with a fresh one.
fn next_spark<'a>(
    gauges: &'a [Gauge],
    order: &[usize],
    fresh: &dyn Fn(&str) -> bool,
) -> Option<&'a Gauge> {
    let reach: Vec<&Gauge> = order
        .iter()
        .take(SPARK_REACH)
        .map(|&i| &gauges[i])
        .collect();
    let urgent = |g: &Gauge| flooding(g.cat) || flooding(g.forecast_cat);
    reach
        .iter()
        .filter(|g| urgent(g))
        .chain(reach.iter().filter(|g| !urgent(g)))
        .find(|g| !fresh(&g.lid))
        .copied()
}

impl Dashboard {
    /// Put a gauge's observed record in, from a sparkline fetch or the selected gauge's card.
    fn ingest(&mut self, lid: &str, readings: &[Reading]) {
        let newest = readings.last().map(|r| r.time);
        let have = self
            .sparks
            .get(lid)
            .and_then(|s| s.readings.last().map(|r| r.time));
        if newest.is_some() && newest > have {
            self.sparks.insert(
                lid.to_string(),
                Spark {
                    readings: readings.to_vec(),
                    fetched: Instant::now(),
                },
            );
        }
    }

    fn fresh(&self, lid: &str) -> bool {
        self.in_flight.as_deref() == Some(lid)
            || self
                .sparks
                .get(lid)
                .is_some_and(|s| s.fetched.elapsed().as_secs() < SPARK_MAX_AGE_SECS)
    }

    /// Land sparkline fetches and start the next one when it is time.
    fn tick(
        &mut self,
        gauges: &[Gauge],
        order: &[usize],
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
        ctx: &egui::Context,
    ) {
        for (lid, res) in self.rx.try_iter().collect::<Vec<_>>() {
            if self.in_flight.as_deref() == Some(lid.as_str()) {
                self.in_flight = None;
            }
            match res {
                Ok(readings) => {
                    self.sparks.insert(
                        lid,
                        Spark {
                            readings,
                            fetched: Instant::now(),
                        },
                    );
                }
                Err(e) => {
                    log::debug!("gauge sparkline {lid}: {e}");
                    if e.contains("rate limiting") {
                        self.rate_limited = Some(Instant::now());
                    } else {
                        // Keep what there was; either way, not again until it is due.
                        let readings = self.sparks.remove(&lid).map(|s| s.readings);
                        self.sparks.insert(
                            lid,
                            Spark {
                                readings: readings.unwrap_or_default(),
                                fetched: Instant::now(),
                            },
                        );
                    }
                }
            }
        }
        let waiting = self.in_flight.is_some()
            || self
                .last_spark
                .is_some_and(|t| t.elapsed().as_secs() < SPARK_GAP_SECS)
            || self
                .rate_limited
                .is_some_and(|t| t.elapsed().as_secs() < SPARK_BACKOFF_SECS);
        if !waiting {
            let next = next_spark(gauges, order, &|lid| self.fresh(lid)).map(|g| g.lid.clone());
            if let Some(lid) = next {
                self.last_spark = Some(Instant::now());
                self.in_flight = Some(lid.clone());
                let (tx, http, ctx) = (self.tx.clone(), http.clone(), ctx.clone());
                spawner.spawn(async move {
                    let res = wxdata::river::fetch_hydrograph(&http, &lid)
                        .await
                        .map(|h| h.observed.readings)
                        .map_err(|e| e.to_string());
                    let _ = tx.send((lid, res));
                    ctx.request_repaint();
                });
            }
        }
        // The next sparkline is due on a clock, not on input.
        ctx.request_repaint_after(std::time::Duration::from_secs(SPARK_GAP_SECS));
    }
}

/// What the dashboard asks of the app.
pub enum Out {
    /// Switch the map's river-gauge layer on: the list is its fetch.
    ShowLayer,
    Center {
        lat: f64,
        lon: f64,
    },
    /// Something the selected gauge's card asked for.
    Card(gauge_card::Action),
}

/// What the dashboard is drawn with.
pub struct Env<'a> {
    pub tz: Option<Tz>,
    pub spawner: &'a crate::rt::Spawner,
    pub http: &'a reqwest::Client,
    /// The map's river-gauge layer is on.
    pub layer_on: bool,
    /// The map is zoomed out past where gauges are fetched.
    pub zoomed_out: bool,
    /// Floating: the table and the page keep to a height rather than filling a dock.
    pub max_table_h: f32,
}

/// Draw the dashboard.
pub fn body(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    dash: &mut Dashboard,
    gauges: &[Gauge],
    cards: &mut Cards,
    env: Env<'_>,
) -> Vec<Out> {
    let mut out = Vec::new();
    let now = Utc::now();
    // The selected gauge's week feeds its sparkline too, so selecting one fills its row at once.
    if let (Some(lid), Some(h)) = (cards.focus_lid().map(str::to_string), cards.focus_hydro()) {
        let readings = h.observed.readings.clone();
        dash.ingest(&lid, &readings);
    }
    if !env.layer_on {
        ui.add_space(4.0);
        ui.label(ws::text(
            "The dashboard lists the gauges the map's river-gauge layer fetches for the view.",
            12.0,
            t.text_dim,
        ));
        ui.add_space(4.0);
        if ws::button(ui, t, "Show river gauges", 150.0).clicked() {
            out.push(Out::ShowLayer);
        }
        return out;
    }
    if gauges.is_empty() {
        ui.add_space(4.0);
        ui.label(ws::text(
            if env.zoomed_out {
                "Zoom in to a region (under about 12\u{b0} wide) to list its river gauges."
            } else {
                "Asking the river forecast service for the gauges in view\u{2026}"
            },
            12.0,
            t.text_dim,
        ));
    } else {
        let stats: HashMap<&str, RowStats> = gauges
            .iter()
            .filter_map(|g| {
                let s = dash.sparks.get(&g.lid)?;
                Some((g.lid.as_str(), RowStats::of(&s.readings, now)))
            })
            .collect();
        let summary = summarize(gauges);
        overview(ui, t, dash, gauges, &summary, &stats);
        week_chart(ui, t, dash, gauges, &stats, env.tz, now);
        let order = rows(gauges, &stats, dash.filter, dash.sort, &dash.query);
        controls(ui, t, dash, &summary, &stats, gauges.len());
        table(
            ui, t, dash, gauges, &order, &stats, cards, &env, &mut out, now,
        );
        let urgent: Vec<usize> = rows(gauges, &stats, Filter::All, Sort::Severity, "");
        let spark_order = if order.is_empty() { &urgent } else { &order };
        dash.tick(gauges, spark_order, env.spawner, env.http, ui.ctx());
        let tracked = gauges
            .iter()
            .filter(|g| dash.sparks.contains_key(&g.lid))
            .count();
        ui.label(ws::text(
            format!(
                "Sparklines for {tracked} of {} gauges; one more a minute, worst first.",
                gauges.len()
            ),
            10.5,
            t.text_faint,
        ));
    }
    ui.add_space(6.0);
    selected(ui, t, cards, &env, &mut out);
    ui.add_space(4.0);
    ui.label(ws::text(
        "Data: NOAA National Water Prediction Service",
        10.0,
        t.text_faint,
    ));
    out
}

/// The category bar, its legend, and the headline counts.
fn overview(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    dash: &mut Dashboard,
    gauges: &[Gauge],
    s: &Summary,
    stats: &HashMap<&str, RowStats>,
) {
    ws::section_rule(ui, t, "Overview");
    // Headline tiles.
    let rising = stats.values().filter(|r| r.rising()).count();
    let tiles = [
        (
            "In flood",
            s.in_flood().to_string(),
            if s.in_flood() > 0 { t.danger } else { t.text },
            "At minor flooding or worse now",
        ),
        (
            "Action",
            s.counts[FloodCat::Action.severity() as usize].to_string(),
            if s.counts[3] > 0 { t.warn } else { t.text },
            "At action stage: near flooding, not yet in it",
        ),
        (
            "Forecast",
            s.forecast_flooding.to_string(),
            if s.worsening > 0 { t.warn } else { t.text },
            "Forecast to be at action stage or worse",
        ),
        (
            "Rising",
            format!("{rising}/{}", stats.len()),
            t.text,
            "Rising over the last three hours, of the gauges with a sparkline",
        ),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let w = ((ui.available_width() - 18.0) / 4.0).clamp(64.0, 120.0);
        for (label, value, color, tip) in tiles {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 42.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 0.0, t.field);
            p.text(
                r.left_top() + egui::vec2(7.0, 5.0),
                egui::Align2::LEFT_TOP,
                label,
                FontId::proportional(10.5),
                t.text_dim,
            );
            p.text(
                r.left_bottom() + egui::vec2(7.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                value,
                FontId::monospace(17.0),
                color,
            );
            resp.on_hover_text(tip);
        }
    });
    ui.add_space(6.0);
    // The bar: one segment per category, as wide as its share; a click filters the table to it.
    let total = s.total().max(1) as f32;
    let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), Sense::hover());
    let mut x = bar.left();
    for (i, c) in CATS.iter().enumerate() {
        let n = s.counts[i];
        if n == 0 {
            continue;
        }
        let w = bar.width() * n as f32 / total;
        let r = Rect::from_min_size(egui::pos2(x, bar.top()), egui::vec2(w, bar.height()));
        x += w;
        let resp = ui
            .interact(r, ui.id().with(("gauge_bar", i)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!(
                "{n} gauge{} at {}\nClick to list only these",
                if n == 1 { "" } else { "s" },
                cat_label(*c)
            ));
        let on = dash.filter == Filter::Category(*c);
        ui.painter().rect_filled(
            r.shrink2(egui::vec2(0.5, 0.0)),
            2.0,
            cat_color(*c).gamma_multiply(if on || resp.hovered() { 1.0 } else { 0.8 }),
        );
        if on {
            ui.painter().rect_stroke(
                r,
                0.0,
                Stroke::new(1.5, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        if resp.clicked() {
            dash.filter = if on {
                Filter::All
            } else {
                Filter::Category(*c)
            };
        }
    }
    // Legend: the categories present, with their counts.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 2.0);
        for (i, c) in CATS.iter().enumerate() {
            let n = s.counts[i];
            if n == 0 {
                continue;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ws::status_dot(ui, cat_color(*c), 4.0);
                ui.label(ws::text(format!("{n} {}", short_cat(*c)), 11.0, t.text_dim));
            });
        }
    });
    // The standouts.
    let worst = gauges
        .iter()
        .filter(|g| flooding(g.cat))
        .min_by_key(|g| g.cat.severity());
    if let Some(g) = worst {
        ui.label(ws::text(
            format!(
                "Worst: {} ({}) at {}{}",
                g.name,
                g.lid,
                cat_label(g.cat),
                g.stage_ft
                    .map(|s| format!(", {s:.1} ft"))
                    .unwrap_or_default()
            ),
            11.5,
            cat_color(g.cat),
        ));
    }
    let fastest = gauges
        .iter()
        .filter_map(|g| Some((g, stats.get(g.lid.as_str())?.change_24h?)))
        .filter(|(_, d)| *d >= 0.1)
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((g, d)) = fastest {
        ui.label(ws::text(
            format!("Fastest rise: {} ({}) {d:+.1} ft in 24 h", g.name, g.lid),
            11.5,
            t.text,
        ));
    }
    if s.worsening > 0 {
        ui.label(ws::text(
            format!(
                "{} gauge{} forecast to reach a worse category than now",
                s.worsening,
                if s.worsening == 1 { " is" } else { "s are" }
            ),
            11.5,
            t.warn,
        ));
    }
}

fn short_cat(c: FloodCat) -> &'static str {
    match c {
        FloodCat::Major => "major",
        FloodCat::Moderate => "moderate",
        FloodCat::Minor => "minor",
        FloodCat::Action => "action",
        FloodCat::NoFlooding => "normal",
        FloodCat::Unknown => "no reading",
    }
}

/// The worst gauges' change over the week, one line each, on one chart: stages of different
/// rivers are not comparable in feet, their rises are.
fn week_chart(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    dash: &Dashboard,
    gauges: &[Gauge],
    stats: &HashMap<&str, RowStats>,
    tz: Option<Tz>,
    now: DateTime<Utc>,
) {
    let mut picks: Vec<&Gauge> = gauges
        .iter()
        .filter(|g| {
            dash.sparks
                .get(&g.lid)
                .is_some_and(|s| s.readings.len() >= 2)
        })
        .collect();
    let rise = |g: &Gauge| {
        stats
            .get(g.lid.as_str())
            .and_then(|s| s.change_24h)
            .map_or(0.0, f64::abs)
    };
    picks.sort_by(|a, b| {
        (a.cat.severity().min(a.forecast_cat.severity()))
            .cmp(&b.cat.severity().min(b.forecast_cat.severity()))
            .then(rise(b).total_cmp(&rise(a)))
    });
    picks.truncate(CHART_LINES);
    let lines: Vec<(String, Vec<(f64, f32)>)> = picks
        .iter()
        .filter_map(|g| {
            let week: Vec<&Reading> = dash.sparks[&g.lid]
                .readings
                .iter()
                .filter(|r| r.time >= now - Duration::hours(SPARK_HOURS) && r.stage_ft.is_some())
                .collect();
            let base = week.first()?.stage_ft?;
            // A reading an hour is plenty for a week on a dock-wide chart.
            let mut pts: Vec<(f64, f32)> = Vec::new();
            for r in week {
                let x = r.time.timestamp() as f64;
                if pts.last().is_some_and(|p| x - p.0 < 3600.0) {
                    continue;
                }
                pts.push((x, (r.stage_ft? - base) as f32));
            }
            (pts.len() >= 2).then(|| (g.lid.clone(), pts))
        })
        .collect();
    if lines.is_empty() {
        return;
    }
    const COLORS: [Color32; CHART_LINES] = [
        Color32::from_rgb(90, 160, 255),
        Color32::from_rgb(255, 150, 70),
        Color32::from_rgb(110, 210, 120),
        Color32::from_rgb(220, 120, 220),
        Color32::from_rgb(240, 200, 40),
    ];
    let series: Vec<ws::Series> = lines
        .iter()
        .zip(COLORS)
        .map(|((name, pts), color)| ws::Series {
            name,
            color,
            points: pts,
        })
        .collect();
    ui.add_space(4.0);
    ws::series_chart(
        ui,
        t,
        "gauge_week_chart",
        "Change over the past week, worst gauges",
        "ft",
        &series,
        &|x| {
            DateTime::from_timestamp(x as i64, 0).map_or_else(String::new, |tm| match tz {
                Some(z) => tm.with_timezone(&z).format("%a %-I %p").to_string(),
                None => tm.format("%a %HZ").to_string(),
            })
        },
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 2.0);
        for s in &series {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ws::status_dot(ui, s.color, 3.5);
                ui.label(ws::mono(s.name, 10.5, t.text_dim));
            });
        }
    });
}

/// Filter chips, search, sort.
fn controls(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    dash: &mut Dashboard,
    s: &Summary,
    stats: &HashMap<&str, RowStats>,
    total: usize,
) {
    ws::section_rule(ui, t, "Gauges");
    let rising = stats.values().filter(|r| r.rising()).count();
    let filters = [
        (Filter::All, format!("All {total}")),
        (
            Filter::Flooding,
            format!("Flooding {}", s.in_flood() + s.counts[3]),
        ),
        (
            Filter::Forecast,
            format!("Forecast {}", s.forecast_flooding),
        ),
        (Filter::Rising, format!("Rising {rising}")),
    ];
    let mut labels: Vec<String> = filters.iter().map(|(_, l)| l.clone()).collect();
    if let Filter::Category(c) = dash.filter {
        labels.push(format!("{} \u{2715}", short_cat(c)));
    }
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let selected = filters
        .iter()
        .position(|(f, _)| *f == dash.filter)
        .or((matches!(dash.filter, Filter::Category(_))).then_some(filters.len()));
    if let Some(i) = ws::chips(ui, t, &refs, selected) {
        dash.filter = filters.get(i).map_or(Filter::All, |(f, _)| *f);
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let sel = Sort::ALL.iter().position(|s| *s == dash.sort).unwrap_or(0);
        let names: Vec<&str> = Sort::ALL.iter().map(|s| s.label()).collect();
        if let Some(i) = ws::segmented(ui, t, &names, sel) {
            dash.sort = Sort::ALL[i];
        }
        ui.add(
            egui::TextEdit::singleline(&mut dash.query)
                .hint_text("Search name or id")
                .desired_width(ui.available_width().max(60.0)),
        );
    });
    ui.add_space(2.0);
}

/// Cut `s` to fit `width` in `font`, with an ellipsis.
fn fit(ui: &egui::Ui, s: &str, font: &FontId, width: f32) -> String {
    let w = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if w(s) <= width {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut: String = chars[..mid].iter().collect::<String>() + "\u{2026}";
        if w(&cut) <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "\u{2026}"
}

#[allow(clippy::too_many_arguments)]
fn table(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    dash: &Dashboard,
    gauges: &[Gauge],
    order: &[usize],
    stats: &HashMap<&str, RowStats>,
    cards: &mut Cards,
    env: &Env<'_>,
    out: &mut Vec<Out>,
    now: DateTime<Utc>,
) {
    if order.is_empty() {
        ui.label(ws::text("No gauges match.", 12.0, t.text_dim));
        return;
    }
    // Columns: dot, id, name (whatever is left), stage, forecast, week sparkline, 24 h change.
    const DOT: f32 = 16.0;
    const LID: f32 = 50.0;
    const STAGE: f32 = 46.0;
    const FCST: f32 = 50.0;
    const SPARK: f32 = 60.0;
    const CHANGE: f32 = 44.0;
    let avail = ui.available_width();
    let fixed = 8.0 + DOT + LID + STAGE + FCST + CHANGE + 6.0;
    let show_spark = avail - fixed - SPARK >= 70.0;
    let name_w = (avail - fixed - if show_spark { SPARK + 6.0 } else { 0.0 }).max(30.0);
    let cols: Vec<(&str, f32, &str)> = [
        ("", DOT, ""),
        ("ID", LID, "NWS gauge id"),
        ("Name", name_w, "River and place"),
        ("Stage", STAGE, "Latest observed stage, ft"),
        ("Fcst", FCST, "Forecast crest, ft, colored by its category"),
    ]
    .into_iter()
    .chain(show_spark.then_some(("Week", SPARK + 6.0, "Observed stage over the past week")))
    .chain(std::iter::once((
        "24 h",
        CHANGE,
        "Change over the last 24 hours, ft",
    )))
    .collect();
    let (hr, _) = ui.allocate_exact_size(egui::vec2(avail, ROW_H), Sense::hover());
    ui.painter().rect_filled(hr, 0.0, t.panel_hi);
    let mut x = hr.left() + 8.0;
    for (i, (label, w, tip)) in cols.iter().enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, hr.top()), egui::vec2(*w, ROW_H));
        x += w;
        if !tip.is_empty() {
            ui.interact(r, ui.id().with(("gauge_col", i)), Sense::hover())
                .on_hover_text(*tip);
        }
        ui.painter().text(
            r.left_center(),
            egui::Align2::LEFT_CENTER,
            *label,
            FontId::proportional(11.0),
            t.text_dim,
        );
    }
    let focus = cards.focus_lid().map(str::to_string);
    let font = FontId::proportional(11.5);
    let mut pick: Option<(usize, bool)> = None;
    egui::ScrollArea::vertical()
        .id_salt("gauge_dash_rows")
        .max_height(env.max_table_h)
        .auto_shrink([false, true])
        .show_rows(ui, ROW_H, order.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for n in range {
                let i = order[n];
                let g = &gauges[i];
                let st = stats.get(g.lid.as_str()).copied().unwrap_or_default();
                let (r, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
                let on = focus.as_deref() == Some(g.lid.as_str());
                let p = ui.painter();
                if on {
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
                let y = r.center().y;
                p.circle_filled(egui::pos2(x + 5.0, y), 4.5, cat_color(g.cat));
                if worsening(g) {
                    p.circle_stroke(
                        egui::pos2(x + 5.0, y),
                        6.5,
                        Stroke::new(1.2, cat_color(g.forecast_cat)),
                    );
                }
                x += DOT;
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    &g.lid,
                    FontId::monospace(11.0),
                    Color32::WHITE,
                );
                x += LID;
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    fit(ui, &g.name, &font, name_w - 6.0),
                    font.clone(),
                    t.text,
                );
                x += name_w;
                let dash_s = "\u{2014}".to_string();
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    g.stage_ft.map_or(dash_s.clone(), |s| format!("{s:.1}")),
                    FontId::monospace(11.0),
                    if flooding(g.cat) {
                        cat_color(g.cat)
                    } else {
                        t.text
                    },
                );
                x += STAGE;
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    g.forecast_ft.map_or(dash_s.clone(), |s| format!("{s:.1}")),
                    FontId::monospace(11.0),
                    if flooding(g.forecast_cat) {
                        cat_color(g.forecast_cat)
                    } else {
                        t.text_dim
                    },
                );
                x += FCST;
                if show_spark {
                    let sr = Rect::from_min_size(
                        egui::pos2(x, r.top() + 4.0),
                        egui::vec2(SPARK, ROW_H - 8.0),
                    );
                    if let Some(s) = dash.sparks.get(&g.lid) {
                        sparkline(p, sr, &s.readings, now, cat_color(g.cat), t);
                    } else {
                        p.text(
                            sr.left_center(),
                            egui::Align2::LEFT_CENTER,
                            "\u{b7}\u{b7}\u{b7}",
                            FontId::proportional(10.0),
                            t.text_faint,
                        );
                    }
                    x += SPARK + 6.0;
                }
                let (txt, col) = match st.change_24h {
                    Some(d) if d >= 0.1 => (format!("{d:+.1}"), t.warn),
                    Some(d) if d <= -0.1 => (format!("{d:+.1}"), t.live),
                    Some(_) => ("0.0".to_string(), t.text_dim),
                    None => (dash_s, t.text_faint),
                };
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    txt,
                    FontId::monospace(11.0),
                    col,
                );
                let mut tip = format!("{} ({})\n{}", g.name, g.lid, cat_label(g.cat));
                if let Some(s) = g.stage_ft {
                    tip.push_str(&format!(", {s:.2} ft"));
                }
                if let Some(f) = g.forecast_ft {
                    tip.push_str(&format!(
                        "\nForecast {f:.1} ft ({})",
                        cat_label(g.forecast_cat)
                    ));
                }
                match st.trend {
                    Some(Trend::Rising(r)) => tip.push_str(&format!("\nRising {r:.2} ft/h")),
                    Some(Trend::Falling(r)) => tip.push_str(&format!("\nFalling {r:.2} ft/h")),
                    Some(Trend::Steady) => tip.push_str("\nSteady"),
                    None => {}
                }
                if let Some((lo, hi)) = st.range {
                    tip.push_str(&format!("\nPast week {lo:.1} to {hi:.1} ft"));
                }
                tip.push_str("\nClick for its hydrograph; double-click to center the map");
                let resp = resp.on_hover_text(tip);
                if resp.double_clicked() {
                    pick = Some((i, true));
                } else if resp.clicked() {
                    pick = Some((i, false));
                }
            }
        });
    if let Some((i, center)) = pick {
        let g = &gauges[i];
        cards.set_focus(
            &g.lid,
            &g.name,
            (g.lat, g.lon),
            env.spawner,
            env.http,
            ui.ctx(),
        );
        if center {
            out.push(Out::Center {
                lat: g.lat,
                lon: g.lon,
            });
        }
    }
}

/// A week of stage in a small box, scaled to itself.
fn sparkline(
    p: &egui::Painter,
    r: Rect,
    readings: &[Reading],
    now: DateTime<Utc>,
    color: Color32,
    t: &ws::Tokens,
) {
    let x0 = now - Duration::hours(SPARK_HOURS);
    let pts: Vec<(DateTime<Utc>, f64)> = readings
        .iter()
        .filter(|q| q.time >= x0)
        .filter_map(|q| Some((q.time, q.stage_ft?)))
        .collect();
    if pts.len() < 2 {
        p.text(
            r.left_center(),
            egui::Align2::LEFT_CENTER,
            "no data",
            FontId::proportional(9.5),
            t.text_faint,
        );
        return;
    }
    let (lo, hi) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
            (a.min(q.1), b.max(q.1))
        });
    let (lo, hi) = if hi - lo < 0.2 {
        let m = (hi + lo) / 2.0;
        (m - 0.1, m + 0.1)
    } else {
        (lo, hi)
    };
    let span = (now - x0).num_seconds() as f32;
    let line: Vec<egui::Pos2> = pts
        .iter()
        .map(|(tm, v)| {
            egui::pos2(
                r.left() + r.width() * (*tm - x0).num_seconds() as f32 / span,
                r.bottom() - r.height() * ((v - lo) / (hi - lo)) as f32,
            )
        })
        .collect();
    let color = if color == cat_color(FloodCat::Unknown) {
        t.text_dim
    } else {
        color
    };
    let last = *line.last().unwrap_or(&r.center());
    p.add(egui::Shape::line(line, Stroke::new(1.2, color)));
    p.circle_filled(last, 1.8, color);
}

/// The selected gauge's card, or how to get one.
fn selected(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    cards: &mut Cards,
    env: &Env<'_>,
    out: &mut Vec<Out>,
) {
    ws::section_rule(ui, t, "Selected gauge");
    let Some(lid) = cards.focus_lid().map(str::to_string) else {
        ui.label(ws::text(
            "Select a gauge for its hydrograph (up to 30 days), flood stages, forecast, crest \
             history and impacts.",
            11.5,
            t.text_dim,
        ));
        return;
    };
    let mut close = false;
    let mut pop = false;
    ui.horizontal(|ui| {
        ui.label(ws::mono(&lid, 12.0, Color32::WHITE));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ws::icon_button(ui, t, egui_phosphor::regular::X, "", false)
                .on_hover_text("Clear the selection")
                .clicked();
            pop = ws::icon_button(
                ui,
                t,
                egui_phosphor::regular::ARROW_SQUARE_OUT,
                "Pop out",
                false,
            )
            .on_hover_text("Open this gauge in its own card window")
            .clicked();
        });
    });
    if close {
        cards.clear_focus();
        return;
    }
    if pop {
        cards.open(&lid, "", (0.0, 0.0), env.spawner, env.http, ui.ctx());
    }
    for a in cards.show_focus(ui, env.tz, env.spawner, env.http) {
        out.push(Out::Card(a));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(lid: &str, cat: FloodCat, stage: Option<f64>, fcst: FloodCat) -> Gauge {
        Gauge {
            lid: lid.into(),
            name: format!("{lid} River"),
            lat: 30.0,
            lon: -97.0,
            cat,
            stage_ft: stage,
            forecast_ft: None,
            forecast_cat: fcst,
        }
    }

    fn readings(stages: &[f64]) -> Vec<Reading> {
        let t0 = Utc::now() - Duration::hours(stages.len() as i64 - 1);
        stages
            .iter()
            .enumerate()
            .map(|(i, s)| Reading {
                time: t0 + Duration::hours(i as i64),
                stage_ft: Some(*s),
                flow: None,
            })
            .collect()
    }

    fn sample() -> Vec<Gauge> {
        vec![
            g("NORM1", FloodCat::NoFlooding, Some(3.0), FloodCat::Minor),
            g("MAJ1", FloodCat::Major, Some(30.0), FloodCat::Major),
            g("ACT1", FloodCat::Action, Some(12.0), FloodCat::Unknown),
            g("DRY1", FloodCat::Unknown, None, FloodCat::Unknown),
            g("MIN1", FloodCat::Minor, Some(18.0), FloodCat::Moderate),
        ]
    }

    #[test]
    fn the_summary_counts_each_category_and_what_is_coming() {
        let s = summarize(&sample());
        assert_eq!(s.counts, [1, 0, 1, 1, 1, 1]);
        assert_eq!(s.total(), 5);
        assert_eq!(s.in_flood(), 2, "minor and worse");
        // NORM1 (to minor), MAJ1 (stays major), MIN1 (to moderate).
        assert_eq!(s.forecast_flooding, 3);
        assert_eq!(s.worsening, 2, "staying major is not worsening");
    }

    #[test]
    fn change_reaches_back_as_far_as_asked_and_no_further() {
        let r = readings(&[10.0, 10.5, 11.0, 12.5]);
        assert_eq!(change_over(&r, 3), Some(2.5));
        assert_eq!(change_over(&r, 1), Some(1.5));
        assert_eq!(change_over(&r, 24), None, "a three-hour record has no day");
        let s = RowStats::of(&r, Utc::now());
        assert!(s.rising());
        assert_eq!(s.range, Some((10.0, 12.5)));
    }

    #[test]
    fn rows_filter_search_and_sort() {
        let gs = sample();
        let lids = |v: Vec<usize>| -> Vec<&str> { v.iter().map(|&i| gs[i].lid.as_str()).collect() };
        let none = HashMap::new();
        assert_eq!(
            lids(rows(&gs, &none, Filter::All, Sort::Severity, "")),
            ["MAJ1", "MIN1", "ACT1", "NORM1", "DRY1"]
        );
        assert_eq!(
            lids(rows(&gs, &none, Filter::Flooding, Sort::Severity, "")),
            ["MAJ1", "MIN1", "ACT1"]
        );
        assert_eq!(
            lids(rows(&gs, &none, Filter::Forecast, Sort::Name, "")),
            ["MAJ1", "MIN1", "NORM1"]
        );
        assert_eq!(
            lids(rows(
                &gs,
                &none,
                Filter::Category(FloodCat::Action),
                Sort::Name,
                ""
            )),
            ["ACT1"]
        );
        assert_eq!(
            lids(rows(&gs, &none, Filter::All, Sort::Stage, "")),
            ["MAJ1", "MIN1", "ACT1", "NORM1", "DRY1"],
            "no stage sorts last"
        );
        assert_eq!(
            lids(rows(&gs, &none, Filter::All, Sort::Name, "min river")),
            ["MIN1"]
        );
        // Rising and the 24 h sort read the sparklines.
        let rising = RowStats::of(&readings(&[1.0, 2.0, 3.0, 4.0]), Utc::now());
        let day: Vec<f64> = (0..26).map(|h| 5.0 - h as f64 * 0.1).collect();
        let falling = RowStats::of(&readings(&day), Utc::now());
        let stats: HashMap<&str, RowStats> = [("ACT1", rising), ("NORM1", falling)].into();
        assert_eq!(
            lids(rows(&gs, &stats, Filter::Rising, Sort::Severity, "")),
            ["ACT1"]
        );
        assert_eq!(
            lids(rows(&gs, &stats, Filter::All, Sort::Change, ""))[0],
            "NORM1",
            "the only gauge with a day of record leads; the rest have none"
        );
    }

    #[test]
    fn sparklines_go_to_flooding_gauges_first_and_skip_fresh_ones() {
        let gs = sample();
        let order = rows(&gs, &HashMap::new(), Filter::All, Sort::Name, "");
        // In name order ACT1 comes first; the flooding ones still lead, in that order.
        let next = next_spark(&gs, &order, &|_| false).unwrap();
        assert_eq!(next.lid, "ACT1");
        let next = next_spark(&gs, &order, &|l| l == "ACT1").unwrap();
        assert_eq!(next.lid, "MAJ1");
        // NORM1 is forecast to flood, so it is urgent too, ahead of DRY1.
        let urgent_done = |l: &str| ["ACT1", "MAJ1", "MIN1"].contains(&l);
        assert_eq!(next_spark(&gs, &order, &urgent_done).unwrap().lid, "NORM1");
        assert!(next_spark(&gs, &order, &|_| true).is_none());
    }

    /// Draw the dashboard for three frames of a dock-sized screen; every string drawn in the last.
    fn draw(dash: &mut Dashboard, gauges: &[Gauge], layer_on: bool) -> Vec<String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // A current-thread runtime that is never driven: a sparkline fetch is queued, not sent.
        let spawner = crate::rt::Spawner::new(rt.handle().clone());
        let http = reqwest::Client::new();
        let mut cards = Cards::default();
        let t = ws::Tokens::new(Color32::from_rgb(70, 130, 230));
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(440.0, 1400.0),
                )),
                ..Default::default()
            };
            let o = ctx.run_ui(input, |ui| {
                ws::style_scope(ui, &t);
                body(
                    ui,
                    &t,
                    dash,
                    gauges,
                    &mut cards,
                    Env {
                        tz: None,
                        spawner: &spawner,
                        http: &http,
                        layer_on,
                        zoomed_out: false,
                        max_table_h: 300.0,
                    },
                );
            });
            out = o
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect();
        }
        out
    }

    #[test]
    fn the_dashboard_draws_its_overview_chart_and_table() {
        let mut dash = Dashboard::default();
        let day: Vec<f64> = (0..30).map(|h| 20.0 + h as f64 * 0.3).collect();
        dash.ingest("MAJ1", &readings(&day));
        dash.ingest("MIN1", &readings(&[15.0, 16.0, 17.0, 18.0]));
        let texts = draw(&mut dash, &sample(), true);
        let has = |s: &str| texts.iter().any(|t| t.contains(s));
        for s in [
            "In flood",
            "Forecast",
            "MAJ1",
            "DRY1",
            "Change over the past week, worst gauges",
            "Fastest rise: MAJ1 River (MAJ1) +7.2 ft in 24 h",
            "Selected gauge",
        ] {
            assert!(has(s), "{s:?} not drawn in {texts:?}");
        }
        // One gauge was queued for a sparkline (the worst one without a fresh one).
        assert!(dash.in_flight.is_some());
        // Filtered to one category, the other gauges leave the table.
        dash.filter = Filter::Category(FloodCat::Action);
        let texts = draw(&mut dash, &sample(), true);
        assert!(texts.iter().any(|t| t == "ACT1"));
        assert!(!texts.iter().any(|t| t == "DRY1"));
    }

    #[test]
    fn with_the_layer_off_it_offers_to_switch_it_on() {
        let texts = draw(&mut Dashboard::default(), &sample(), false);
        assert!(texts.iter().any(|t| t == "Show river gauges"), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "MAJ1"));
    }
}
