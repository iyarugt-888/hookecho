//! River-gauge cards: what a click on a gauge on the map opens.
//!
//! A card asks NWPS for two documents, the gauge's metadata and its hydrograph, and draws the
//! hydrograph itself: the last day of observed stage (three or seven days on request), the river
//! forecast continuing it, the flood stages as colored bands, and the crests marked with their
//! stage and time. Below it: which way the river is going and how fast, how far it is from the
//! next flood stage, what happens at this level in the forecast office's words, the crest record,
//! and the seasonal chance of flooding.
//!
//! Every platform: it is two small JSON documents and egui painting, nothing native.

use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke};
use std::sync::mpsc::{Receiver, Sender};
use wxdata::clock::Instant;
use wxdata::river::{
    FloodCat, GaugeDetail, Hydrograph, PeakKind, Reading, Series, Thresholds, Trend,
};
use wxdata::tz::Tz;

/// An open card refreshes this often. Gauges report every 15 minutes to an hour, and the service
/// allows ten requests in five minutes, so a handful of open cards stays well inside it.
const REFRESH_SECS: u64 = 15 * 60;

/// Two readings further apart than this are a gap in the record, not a straight line.
const GAP_HOURS: i64 = 3;

/// The map color of a flood category, shared with the gauge symbols.
pub fn cat_color(c: FloodCat) -> Color32 {
    match c {
        FloodCat::Major => Color32::from_rgb(170, 60, 220),
        FloodCat::Moderate => Color32::from_rgb(230, 40, 40),
        FloodCat::Minor => Color32::from_rgb(255, 140, 0),
        FloodCat::Action => Color32::from_rgb(240, 200, 40),
        FloodCat::NoFlooding => Color32::from_rgb(80, 200, 220),
        FloodCat::Unknown => Color32::from_gray(150),
    }
}

/// How a flood category reads in a sentence.
pub fn cat_label(c: FloodCat) -> &'static str {
    match c {
        FloodCat::Major => "major flooding",
        FloodCat::Moderate => "moderate flooding",
        FloodCat::Minor => "minor flooding",
        FloodCat::Action => "action stage",
        FloodCat::NoFlooding => "no flooding",
        FloodCat::Unknown => "no current reading",
    }
}

/// A flood stage's name, as a threshold ("Minor 33 ft").
fn stage_name(c: FloodCat) -> &'static str {
    match c {
        FloodCat::Major => "Major",
        FloodCat::Moderate => "Moderate",
        FloodCat::Minor => "Minor",
        FloodCat::Action => "Action",
        FloodCat::NoFlooding | FloodCat::Unknown => "",
    }
}

/// How far back the graph reaches.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Past {
    Day,
    ThreeDays,
    Week,
}

impl Past {
    fn hours(self) -> i64 {
        match self {
            Past::Day => 24,
            Past::ThreeDays => 72,
            Past::Week => 168,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Past::Day => "24 h",
            Past::ThreeDays => "3 days",
            Past::Week => "7 days",
        }
    }
}

type Landed = (
    String,
    Result<Box<GaugeDetail>, String>,
    Result<Hydrograph, String>,
);

/// One open card.
pub struct Card {
    pub lid: String,
    /// The list's name until the metadata lands with the full one.
    name: String,
    lat: f64,
    lon: f64,
    detail: Option<Box<GaugeDetail>>,
    hydro: Option<Hydrograph>,
    error: Option<String>,
    loading: bool,
    fetched: Option<Instant>,
    open: bool,
    past: Past,
    show_forecast: bool,
    show_stages: bool,
    show_flow: bool,
}

/// What a card asks of the app.
pub enum Action {
    /// Center the map on a gauge.
    Center { lat: f64, lon: f64 },
    /// Share (or copy) a link that opens this gauge's card.
    Share { lid: String, lat: f64, lon: f64 },
}

/// Every open gauge card and the channel their fetches report on.
pub struct Cards {
    pub cards: Vec<Card>,
    /// Gauges asked for by a deep link, opened on the next frame (a link is read before there is
    /// a frame to open a window in).
    queued: Vec<String>,
    tx: Sender<Landed>,
    rx: Receiver<Landed>,
}

impl Default for Cards {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            cards: Vec::new(),
            queued: Vec::new(),
            tx,
            rx,
        }
    }
}

impl Cards {
    /// Open a gauge's card, or bring it forward when it is already up. `lat`/`lon` may be zero
    /// for a gauge known only by id (an upstream neighbor); its metadata fills them in.
    pub fn open(
        &mut self,
        lid: &str,
        name: &str,
        (lat, lon): (f64, f64),
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
        ctx: &egui::Context,
    ) {
        if self.cards.iter().any(|c| c.lid == lid) {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, window_id(lid)));
            return;
        }
        self.cards.push(Card {
            lid: lid.to_string(),
            name: if name.is_empty() {
                lid.to_string()
            } else {
                name.to_string()
            },
            lat,
            lon,
            detail: None,
            hydro: None,
            error: None,
            loading: false,
            fetched: None,
            open: true,
            past: Past::Day,
            show_forecast: true,
            show_stages: true,
            show_flow: false,
        });
        self.fetch(lid, spawner, http, ctx);
    }

    /// The gauges with a card up, for the map to ring.
    pub fn is_open(&self, lid: &str) -> bool {
        self.cards.iter().any(|c| c.lid == lid)
    }

    /// Every open card's gauge id, oldest first (for a shared view link).
    pub fn lids(&self) -> Vec<String> {
        self.cards.iter().map(|c| c.lid.clone()).collect()
    }

    /// Open a gauge's card on the next frame.
    pub fn queue(&mut self, lid: &str) {
        self.queued.push(lid.to_string());
    }

    fn fetch(
        &mut self,
        lid: &str,
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
        ctx: &egui::Context,
    ) {
        let Some(card) = self.cards.iter_mut().find(|c| c.lid == lid) else {
            return;
        };
        card.loading = true;
        card.fetched = Some(Instant::now());
        let (tx, http, ctx, lid) = (self.tx.clone(), http.clone(), ctx.clone(), lid.to_string());
        spawner.spawn(async move {
            let (d, h) = futures_util::future::join(
                wxdata::river::fetch_detail(&http, &lid),
                wxdata::river::fetch_hydrograph(&http, &lid),
            )
            .await;
            let _ = tx.send((
                lid,
                d.map(Box::new).map_err(|e| e.to_string()),
                h.map_err(|e| e.to_string()),
            ));
            ctx.request_repaint();
        });
    }

    /// Land finished fetches, draw every card, refresh the stale ones, and drop the closed ones.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        tz: Option<Tz>,
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
    ) -> Vec<Action> {
        for lid in std::mem::take(&mut self.queued) {
            self.open(&lid, "", (0.0, 0.0), spawner, http, ctx);
        }
        for (lid, d, h) in self.rx.try_iter() {
            let Some(card) = self.cards.iter_mut().find(|c| c.lid == lid) else {
                continue;
            };
            card.loading = false;
            // Keep what was there when a refresh fails: an hour-old hydrograph beats a blank one.
            card.error = d.as_ref().err().or(h.as_ref().err()).cloned();
            if let Ok(d) = d {
                card.name = d.summary.name.clone();
                (card.lat, card.lon) = (d.summary.lat, d.summary.lon);
                card.detail = Some(d);
            }
            if let Ok(h) = h {
                card.hydro = Some(h);
            }
        }
        let mut actions = Vec::new();
        let mut neighbors = Vec::new();
        let mut refetch = Vec::new();
        for (i, card) in self.cards.iter_mut().enumerate() {
            if let Some(req) = show_card(ctx, card, tz, i, &mut actions) {
                match req {
                    Request::Refresh => refetch.push(card.lid.clone()),
                    Request::Open(lid) => neighbors.push(lid),
                }
            }
            let stale = card
                .fetched
                .is_none_or(|t| t.elapsed().as_secs() >= REFRESH_SECS);
            if card.open && stale && !card.loading {
                refetch.push(card.lid.clone());
            }
        }
        self.cards.retain(|c| c.open);
        for lid in refetch {
            self.fetch(&lid, spawner, http, ctx);
        }
        for lid in neighbors {
            self.open(&lid, "", (0.0, 0.0), spawner, http, ctx);
        }
        // The age line and the refresh want a tick now and then even with nothing moving.
        if !self.cards.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(30));
        }
        actions
    }
}

enum Request {
    Refresh,
    Open(String),
}

fn window_id(lid: &str) -> egui::Id {
    egui::Id::new(("gauge-card", lid))
}

fn show_card(
    ctx: &egui::Context,
    card: &mut Card,
    tz: Option<Tz>,
    index: usize,
    actions: &mut Vec<Action>,
) -> Option<Request> {
    let mut open = card.open;
    let mut request = None;
    let title = format!("{} ({})", card.name, card.lid);
    let offset = 24.0 * (index % 8) as f32;
    let w = crate::ui::phone_surface(
        ctx,
        egui::Window::new(RichText::new(&title).size(13.0))
            .id(window_id(&card.lid))
            .open(&mut open)
            .default_pos([120.0 + offset, 110.0 + offset]),
    );
    let w = if crate::platform::phone_layout() {
        w
    } else {
        w.default_size([470.0, 640.0]).resizable(true)
    };
    w.show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.8)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                request = body(ui, card, tz, actions);
            });
    });
    card.open = open;
    request
}

fn body(
    ui: &mut egui::Ui,
    card: &mut Card,
    tz: Option<Tz>,
    actions: &mut Vec<Action>,
) -> Option<Request> {
    let mut request = None;
    if let Some(e) = &card.error {
        ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {e}"));
    }
    let Some(d) = card.detail.as_deref() else {
        if card.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Asking the river forecast service…");
            });
        }
        return None;
    };
    header(ui, d, card.hydro.as_ref(), tz);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for p in [Past::Day, Past::ThreeDays, Past::Week] {
            ui.selectable_value(&mut card.past, p, p.label());
        }
        ui.separator();
        ui.checkbox(&mut card.show_forecast, "Forecast");
        ui.checkbox(&mut card.show_stages, "Flood stages");
        ui.checkbox(&mut card.show_flow, "Flow");
    });
    match &card.hydro {
        Some(h) => {
            let opts = GraphOpts {
                past: card.past,
                forecast: card.show_forecast,
                stages: card.show_stages,
                flow: card.show_flow,
            };
            hydrograph(ui, h, &d.thresholds, opts, tz, Utc::now());
            crest_lines(ui, h, &d.thresholds, card.past, tz);
        }
        None if card.loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading the hydrograph…");
            });
        }
        None => {
            ui.label(RichText::new("No hydrograph from the service.").weak());
        }
    }
    ui.separator();
    stages_and_impacts(ui, d, card.hydro.as_ref());
    crest_history(ui, d, tz);
    if let Some(o) = &d.outlook {
        ui.label(
            RichText::new(format!(
                "Chance of flooding this season ({}): minor {} · moderate {} · major {}",
                season_label(&o.interval),
                wxdata::river::chance_percent(&o.minor),
                wxdata::river::chance_percent(&o.moderate),
                wxdata::river::chance_percent(&o.major),
            ))
            .size(11.0),
        );
    }
    if !d.reliability.is_empty() {
        ui.label(RichText::new(&d.reliability).size(10.5).weak());
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if let Some(up) = &d.upstream {
            if ui
                .button(format!("⬆ Upstream {up}"))
                .on_hover_text("Open the next gauge up the river")
                .clicked()
            {
                request = Some(Request::Open(up.clone()));
            }
        }
        if let Some(down) = &d.downstream {
            if ui
                .button(format!("⬇ Downstream {down}"))
                .on_hover_text("Open the next gauge down the river")
                .clicked()
            {
                request = Some(Request::Open(down.clone()));
            }
        }
        if ui.button("⌖ Center map").clicked() {
            actions.push(Action::Center {
                lat: card.lat,
                lon: card.lon,
            });
        }
        if ui
            .button("Copy link")
            .on_hover_text("Copy a link that opens this gauge")
            .clicked()
        {
            actions.push(Action::Share {
                lid: card.lid.clone(),
                lat: card.lat,
                lon: card.lon,
            });
        }
        let refresh = ui.add_enabled(!card.loading, egui::Button::new("⟳ Refresh"));
        if refresh.clicked() {
            request = Some(Request::Refresh);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to("NWPS gauge page", wxdata::river::page_url(&card.lid));
        if !d.usgs_id.is_empty() {
            ui.hyperlink_to(
                format!("USGS {}", d.usgs_id),
                format!(
                    "https://waterdata.usgs.gov/monitoring-location/{}/",
                    d.usgs_id
                ),
            );
        }
        ui.label(
            RichText::new("Data: NOAA National Water Prediction Service")
                .size(10.0)
                .weak(),
        );
    });
    request
}

fn chip(ui: &mut egui::Ui, text: &str, bg: Color32) {
    // Dark text on the light category colors (action yellow, no-flooding cyan), light on the rest.
    let luma = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    let fg = if luma > 150.0 {
        Color32::from_gray(20)
    } else {
        Color32::WHITE
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(fg).strong());
        });
}

/// A flow in the units a reader expects: cfs under ten thousand, kcfs above.
fn flow_text(v: f64, units: &str) -> String {
    let cfs = match units {
        "kcfs" => v * 1000.0,
        "cfs" => v,
        _ => return format!("{v:.2} {units}"),
    };
    if cfs >= 10_000.0 {
        format!("{:.1} kcfs", cfs / 1000.0)
    } else {
        format!("{cfs:.0} cfs")
    }
}

/// `"Tue 4:00 AM"` in the display zone, or `"Tue 09:00Z"`.
fn when(t: DateTime<Utc>, tz: Option<Tz>) -> String {
    match tz {
        Some(z) => t.with_timezone(&z).format("%a %-I:%M %p").to_string(),
        None => t.format("%a %H:%MZ").to_string(),
    }
}

fn age_text(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let m = (now - t).num_minutes().max(0);
    match m {
        0 => "just now".into(),
        1..=89 => format!("{m} min ago"),
        _ => format!("{} h ago", m / 60),
    }
}

/// `SON` as `Sep–Nov`: the three initials of consecutive months.
fn season_label(code: &str) -> String {
    const INITIALS: &[u8] = b"JFMAMJJASOND";
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let c = code.as_bytes();
    if c.len() == 3 {
        for start in 0..12 {
            if (0..3).all(|k| INITIALS[(start + k) % 12] == c[k].to_ascii_uppercase()) {
                return format!("{}–{}", NAMES[start], NAMES[(start + 2) % 12]);
            }
        }
    }
    code.to_string()
}

fn header(ui: &mut egui::Ui, d: &GaugeDetail, h: Option<&Hydrograph>, tz: Option<Tz>) {
    let now = Utc::now();
    let g = &d.summary;
    ui.horizontal_wrapped(|ui| {
        chip(ui, cat_label(g.cat), cat_color(g.cat));
        let place = match (d.county.is_empty(), d.state.is_empty()) {
            (false, false) => format!("{} Co., {}", d.county, d.state),
            (true, false) => d.state.clone(),
            _ => String::new(),
        };
        let offices = [d.wfo.as_str(), d.rfc.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        ui.label(
            RichText::new(format!("{place}   {offices}"))
                .size(10.5)
                .weak(),
        );
    });
    if !d.in_service {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("Out of service. {}", d.service_note),
        );
    }
    // The hydrograph's newest reading is fresher than the list's status block.
    let latest = h.and_then(|h| h.observed.latest().copied());
    let (stage, flow, units, at) = match latest {
        Some(r) => (
            r.stage_ft,
            r.flow,
            h.map_or("", |h| h.observed.flow_units.as_str()),
            Some(r.time),
        ),
        None => (
            g.stage_ft,
            d.observed_flow,
            d.flow_units.as_str(),
            d.observed_time,
        ),
    };
    ui.horizontal_wrapped(|ui| {
        match stage {
            Some(s) => {
                ui.label(RichText::new(format!("{s:.2} ft")).size(22.0).strong());
            }
            None => {
                ui.label(RichText::new("No current stage").size(16.0).weak());
            }
        }
        ui.vertical(|ui| {
            if let Some(f) = flow {
                ui.label(RichText::new(flow_text(f, units)).size(12.0));
            }
            if let Some(t) = at {
                let fresh = (now - t).num_minutes() < 120;
                let col = if fresh {
                    Color32::from_rgb(120, 200, 130)
                } else {
                    Color32::from_rgb(220, 180, 90)
                };
                ui.label(
                    RichText::new(format!("observed {} ({})", when(t, tz), age_text(t, now)))
                        .size(10.5)
                        .color(col),
                );
            }
        });
    });
    // Trend and distance to the next flood stage.
    let mut facts = Vec::new();
    if let Some(h) = h {
        match wxdata::river::trend(&h.observed.readings, 3.0) {
            Some(Trend::Rising(r)) => facts.push(format!("↑ rising {r:.2} ft/h")),
            Some(Trend::Falling(r)) => facts.push(format!("↓ falling {r:.2} ft/h")),
            Some(Trend::Steady) => facts.push("→ steady".to_string()),
            None => {}
        }
        if let (Some(last), Some(s)) = (h.observed.latest(), stage) {
            let day_ago = h
                .observed
                .readings
                .iter()
                .rev()
                .find(|r| r.stage_ft.is_some() && r.time <= last.time - Duration::hours(24));
            if let Some(s0) = day_ago.and_then(|r| r.stage_ft) {
                facts.push(format!("{:+.2} ft in 24 h", s - s0));
            }
        }
    }
    if let Some(s) = stage {
        match d.thresholds.next_above(s) {
            Some((c, at)) => facts.push(format!(
                "{:.1} ft below {} stage ({at:.0} ft)",
                at - s,
                stage_name(c).to_lowercase()
            )),
            None => {
                if let Some((c, at)) = d.thresholds.levels().last() {
                    facts.push(format!(
                        "{:.1} ft above {} stage",
                        s - at,
                        stage_name(*c).to_lowercase()
                    ));
                }
            }
        }
    }
    if !facts.is_empty() {
        ui.label(RichText::new(facts.join("   ")).size(12.0));
    }
}

#[derive(Clone, Copy)]
struct GraphOpts {
    past: Past,
    forecast: bool,
    stages: bool,
    flow: bool,
}

/// The readings the graph shows: observed over the window, and the forecast after the newest
/// observation (the forecast's own first hours overlap what has since been observed), reaching
/// ahead at most twice the window: a five-day forecast would otherwise squeeze the last day into
/// a sixth of the graph. The line under the graph reports the whole forecast's crest.
fn visible(
    h: &Hydrograph,
    past: Past,
    forecast: bool,
    now: DateTime<Utc>,
) -> (&[Reading], &[Reading]) {
    let obs = h.observed.since(now - Duration::hours(past.hours()));
    let fc = if forecast {
        let after = h.observed.latest().map_or(now, |r| r.time);
        let horizon = after + Duration::hours(past.hours() * 2);
        let i = h.forecast.readings.partition_point(|r| r.time <= after);
        let j = h.forecast.readings.partition_point(|r| r.time <= horizon);
        &h.forecast.readings[i..j.max(i)]
    } else {
        &[][..]
    };
    (obs, fc)
}

/// A round axis step giving about `n` ticks over `span`.
fn nice_step(span: f64, n: f64) -> f64 {
    let raw = (span / n).max(1e-6);
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let m = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    m * mag
}

/// The stage range to draw: the data, padded, stretched to show the next flood stage above it so
/// the distance to flooding is on the graph.
fn stage_range(values: &[f64], t: &Thresholds, stages: bool) -> Option<(f64, f64)> {
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !lo.is_finite() {
        return None;
    }
    let mut hi = hi;
    if stages {
        if let Some((_, next)) = t.next_above(hi) {
            hi = next;
        }
    }
    // A near-flat river still gets a foot of room, centered on it, so gauge noise is not drawn
    // as a torrent.
    let (lo, hi) = if hi - lo < 1.0 {
        let mid = (hi + lo) * 0.5;
        (mid - 0.5, mid + 0.5)
    } else {
        (lo, hi)
    };
    let span = hi - lo;
    Some((lo - span * 0.08, hi + span * 0.12))
}

fn hydrograph(
    ui: &mut egui::Ui,
    h: &Hydrograph,
    t: &Thresholds,
    opts: GraphOpts,
    tz: Option<Tz>,
    now: DateTime<Utc>,
) {
    let (obs, fc) = visible(h, opts.past, opts.forecast, now);
    let x0 = now - Duration::hours(opts.past.hours());
    let x1 = fc.last().map_or(now, |r| r.time.max(now));
    let stages: Vec<f64> = obs.iter().chain(fc).filter_map(|r| r.stage_ft).collect();
    let Some((y0, y1)) = stage_range(&stages, t, opts.stages) else {
        ui.label(RichText::new("No stage readings in this window.").weak());
        return;
    };
    let width = ui.available_width().max(260.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 220.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let vis = ui.visuals();
    let text = vis.text_color();
    let weak = vis.weak_text_color();
    let grid = vis
        .widgets
        .noninteractive
        .bg_stroke
        .color
        .gamma_multiply(0.6);
    painter.rect_filled(rect, 4.0, vis.extreme_bg_color);
    let plot = Rect::from_min_max(
        rect.min + egui::vec2(40.0, 8.0),
        rect.max - egui::vec2(if opts.flow { 48.0 } else { 10.0 }, 22.0),
    );
    let span_s = (x1 - x0).num_seconds().max(1) as f64;
    let xs = |tm: DateTime<Utc>| {
        plot.left() + ((tm - x0).num_seconds() as f64 / span_s) as f32 * plot.width()
    };
    let ys = |v: f64| plot.bottom() - ((v - y0) / (y1 - y0)) as f32 * plot.height();
    let small = FontId::proportional(10.0);

    // Flood-stage bands and lines, under everything.
    if opts.stages {
        let levels = t.levels();
        for (i, (c, s)) in levels.iter().enumerate() {
            if *s > y1 {
                break;
            }
            let top = levels.get(i + 1).map_or(y1, |n| n.1.min(y1));
            if top <= y0 {
                continue;
            }
            let band = Rect::from_x_y_ranges(
                plot.x_range(),
                ys(top).max(plot.top())..=ys(s.max(y0)).min(plot.bottom()),
            );
            painter.rect_filled(band, 0.0, cat_color(*c).gamma_multiply(0.10));
            if *s >= y0 {
                let y = ys(*s);
                painter.extend(egui::Shape::dashed_line(
                    &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                    Stroke::new(1.0, cat_color(*c).gamma_multiply(0.9)),
                    5.0,
                    4.0,
                ));
                painter.text(
                    Pos2::new(plot.left() + 3.0, y - 1.0),
                    Align2::LEFT_BOTTOM,
                    format!("{} {s:.0} ft", stage_name(*c)),
                    small.clone(),
                    cat_color(*c),
                );
            }
        }
    }

    // Stage axis.
    let step = nice_step(y1 - y0, 5.0);
    let mut v = (y0 / step).ceil() * step;
    while v <= y1 {
        let y = ys(v);
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.5, grid),
        );
        let label = if step < 1.0 {
            format!("{v:.1}")
        } else {
            format!("{v:.0}")
        };
        painter.text(
            Pos2::new(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            label,
            small.clone(),
            weak,
        );
        v += step;
    }
    painter.text(
        Pos2::new(rect.left() + 3.0, rect.top() + 2.0),
        Align2::LEFT_TOP,
        "ft",
        small.clone(),
        weak,
    );

    // Time axis: ticks on round local hours.
    let span_h = span_s / 3600.0;
    let step_h = [1i64, 2, 3, 6, 12, 24, 48]
        .into_iter()
        .find(|s| span_h / *s as f64 <= 8.0)
        .unwrap_or(48);
    let offset_s = tz.map_or(0, |z| {
        use chrono::Offset;
        x0.with_timezone(&z).offset().fix().local_minus_utc() as i64
    });
    let step_s = step_h * 3600;
    let mut tick = ((x0.timestamp() + offset_s) / step_s + 1) * step_s - offset_s;
    while tick < x1.timestamp() {
        let Some(tm) = DateTime::from_timestamp(tick, 0) else {
            break;
        };
        let x = xs(tm);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(0.5, grid),
        );
        let (fmt_day, fmt_hour) = match tz {
            Some(_) => ("%a", "%-I %p"),
            None => ("%a", "%HZ"),
        };
        let local = tz.map(|z| tm.with_timezone(&z).naive_local());
        let naive = local.unwrap_or(tm.naive_utc());
        let midnight = chrono::Timelike::hour(&naive) == 0;
        let label = if midnight || step_h >= 24 {
            naive.format(fmt_day).to_string()
        } else {
            naive.format(fmt_hour).to_string()
        };
        painter.text(
            Pos2::new(x, plot.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            if midnight { text } else { weak },
        );
        tick += step_s;
    }

    // Flow on its own axis, behind the stage line.
    if opts.flow {
        let flows: Vec<f64> = obs.iter().chain(fc).filter_map(|r| r.flow).collect();
        let fmax = flows.iter().copied().fold(0.0, f64::max);
        if fmax > 0.0 {
            let units = &h.observed.flow_units;
            let fy = |f: f64| plot.bottom() - (f / (fmax * 1.1)) as f32 * plot.height();
            let col = Color32::from_rgb(140, 170, 110);
            draw_series(
                &painter,
                obs,
                |r| r.flow,
                &xs,
                &fy,
                Stroke::new(1.2, col),
                false,
            );
            draw_series(
                &painter,
                fc,
                |r| r.flow,
                &xs,
                &fy,
                Stroke::new(1.2, col),
                true,
            );
            painter.text(
                Pos2::new(plot.right() + 4.0, plot.top()),
                Align2::LEFT_TOP,
                flow_text(fmax, units),
                small.clone(),
                col,
            );
            painter.text(
                Pos2::new(plot.right() + 4.0, plot.bottom()),
                Align2::LEFT_BOTTOM,
                "flow",
                small.clone(),
                col,
            );
        }
    }

    // Now.
    let xn = xs(now);
    painter.line_segment(
        [Pos2::new(xn, plot.top()), Pos2::new(xn, plot.bottom())],
        Stroke::new(1.0, weak),
    );
    painter.text(
        Pos2::new(xn + 2.0, plot.top()),
        Align2::LEFT_TOP,
        "now",
        small.clone(),
        weak,
    );

    // The river.
    let obs_col = Color32::from_rgb(70, 150, 255);
    let fc_col = Color32::from_rgb(200, 120, 255);
    draw_series(
        &painter,
        obs,
        |r| r.stage_ft,
        &xs,
        &ys,
        Stroke::new(2.0, obs_col),
        false,
    );
    // The forecast carries on from the newest observation, so the two read as one line.
    let joined: Vec<Reading> = obs
        .iter()
        .rev()
        .find(|r| r.stage_ft.is_some())
        .into_iter()
        .chain(fc)
        .copied()
        .collect();
    if !fc.is_empty() {
        draw_series(
            &painter,
            &joined,
            |r| r.stage_ft,
            &xs,
            &ys,
            Stroke::new(2.0, fc_col),
            true,
        );
    }

    // Crests.
    // Each label tries above/below the point and to either side, and takes the first spot that
    // stays in the plot and clear of a label already placed (the observed and forecast crests
    // are often an hour apart at the same height).
    let mark = |r: &Reading, label: String, col: Color32, avoid: Option<Rect>| -> Option<Rect> {
        let s = r.stage_ft?;
        let p = Pos2::new(xs(r.time), ys(s));
        painter.circle(p, 4.0, col, Stroke::new(1.5, vis.extreme_bg_color));
        let galley = painter.layout_no_wrap(label, small.clone(), text);
        let right_first = p.x < plot.center().x + plot.width() * 0.25;
        let candidates = [
            (right_first, true),
            (right_first, false),
            (!right_first, true),
            (!right_first, false),
        ];
        let place = |(right, above): (bool, bool)| {
            let anchor = match (right, above) {
                (true, true) => Align2::LEFT_BOTTOM,
                (true, false) => Align2::LEFT_TOP,
                (false, true) => Align2::RIGHT_BOTTOM,
                (false, false) => Align2::RIGHT_TOP,
            };
            let d = egui::vec2(
                if right { 6.0 } else { -6.0 },
                if above { -4.0 } else { 4.0 },
            );
            anchor.anchor_size(p + d, galley.size())
        };
        let fits = |r: &Rect| {
            plot.expand(2.0).contains_rect(*r) && avoid.is_none_or(|a| !a.intersects(r.expand(2.0)))
        };
        let at = candidates
            .into_iter()
            .map(place)
            .find(fits)
            .unwrap_or_else(|| place(candidates[0]));
        painter.rect_filled(
            at.expand(2.0),
            2.0,
            vis.extreme_bg_color.gamma_multiply(0.85),
        );
        painter.galley(at.min, galley, text);
        Some(at)
    };
    let mut placed = None;
    if let Some((r, kind)) = wxdata::river::peak(obs) {
        let label = match kind {
            PeakKind::Crest => "Crest",
            PeakKind::AtEnd => "High (now)",
            PeakKind::AtStart => "High",
        };
        placed = mark(
            &r,
            format!(
                "{label} {:.2} ft · {}",
                r.stage_ft.unwrap_or(0.0),
                when(r.time, tz)
            ),
            obs_col,
            None,
        );
    }
    if let Some((r, kind)) = wxdata::river::peak(&joined) {
        let label = match kind {
            PeakKind::Crest => Some("Fcst crest"),
            PeakKind::AtEnd => Some("Fcst high"),
            PeakKind::AtStart => None,
        };
        if let Some(label) = label {
            mark(
                &r,
                format!(
                    "{label} {:.1} ft · {}",
                    r.stage_ft.unwrap_or(0.0),
                    when(r.time, tz)
                ),
                fc_col,
                placed,
            );
        }
    }

    // Hover: the reading under the pointer.
    if let Some(hp) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let near = obs
            .iter()
            .map(|r| (r, false))
            .chain(fc.iter().map(|r| (r, true)))
            .filter(|(r, _)| r.stage_ft.is_some())
            .min_by(|a, b| {
                (xs(a.0.time) - hp.x)
                    .abs()
                    .total_cmp(&(xs(b.0.time) - hp.x).abs())
            });
        if let Some((r, is_fc)) = near {
            let s = r.stage_ft.unwrap_or(0.0);
            let p = Pos2::new(xs(r.time), ys(s));
            painter.line_segment(
                [Pos2::new(p.x, plot.top()), Pos2::new(p.x, plot.bottom())],
                Stroke::new(0.8, text.gamma_multiply(0.5)),
            );
            painter.circle_stroke(p, 5.0, Stroke::new(1.5, text));
            let units = if is_fc {
                &h.forecast.flow_units
            } else {
                &h.observed.flow_units
            };
            let mut tip = format!(
                "{}\n{s:.2} ft — {}",
                crate::timefmt::fmt_date_clock(r.time, tz),
                cat_label(t.category(s))
            );
            if let Some(f) = r.flow {
                tip.push_str(&format!("\n{}", flow_text(f, units)));
            }
            tip.push_str(if is_fc { "\nforecast" } else { "\nobserved" });
            response.on_hover_text_at_pointer(tip);
        }
    }
}

/// A polyline through the readings that have a value, broken at gaps in the record.
fn draw_series(
    painter: &egui::Painter,
    readings: &[Reading],
    value: impl Fn(&Reading) -> Option<f64>,
    xs: &impl Fn(DateTime<Utc>) -> f32,
    ys: &impl Fn(f64) -> f32,
    stroke: Stroke,
    dashed: bool,
) {
    let mut run: Vec<Pos2> = Vec::new();
    let mut last_t: Option<DateTime<Utc>> = None;
    let flush = |run: &mut Vec<Pos2>| {
        if run.len() >= 2 {
            if dashed {
                painter.extend(egui::Shape::dashed_line(run, stroke, 6.0, 4.0));
            } else {
                painter.add(egui::Shape::line(std::mem::take(run), stroke));
            }
        }
        run.clear();
    };
    for r in readings {
        let Some(v) = value(r) else {
            flush(&mut run);
            last_t = None;
            continue;
        };
        if last_t.is_some_and(|t| r.time - t > Duration::hours(GAP_HOURS)) {
            flush(&mut run);
        }
        run.push(Pos2::new(xs(r.time), ys(v)));
        last_t = Some(r.time);
    }
    flush(&mut run);
}

/// The crests in words under the graph, with what they mean.
fn crest_lines(ui: &mut egui::Ui, h: &Hydrograph, t: &Thresholds, past: Past, tz: Option<Tz>) {
    let now = Utc::now();
    let obs = h.observed.since(now - Duration::hours(past.hours()));
    if let Some((r, kind)) = wxdata::river::peak(obs) {
        let s = r.stage_ft.unwrap_or(0.0);
        let what = match kind {
            PeakKind::Crest => "Crested",
            PeakKind::AtEnd => "Highest now,",
            PeakKind::AtStart => "Highest at the start of the window,",
        };
        ui.label(
            RichText::new(format!(
                "Past {}: {what} {s:.2} ft at {} ({})",
                past.label(),
                when(r.time, tz),
                cat_label(t.category(s))
            ))
            .size(11.5),
        );
    }
    forecast_line(ui, &h.forecast, h.observed.latest(), t, tz);
}

fn forecast_line(
    ui: &mut egui::Ui,
    fc: &Series,
    latest: Option<&Reading>,
    t: &Thresholds,
    tz: Option<Tz>,
) {
    let now = Utc::now();
    let ahead: Vec<Reading> = latest
        .into_iter()
        .copied()
        .chain(
            fc.readings
                .iter()
                .filter(|r| latest.is_none_or(|l| r.time > l.time))
                .copied(),
        )
        .collect();
    if fc.readings.is_empty() || ahead.len() < 2 {
        ui.label(
            RichText::new("No river forecast is out for this gauge right now.")
                .size(11.0)
                .weak(),
        );
        return;
    }
    let issued = fc
        .issued
        .map(|i| format!(" (issued {})", age_text(i, now)))
        .unwrap_or_default();
    let text = match wxdata::river::peak(&ahead) {
        Some((r, PeakKind::Crest)) => {
            let s = r.stage_ft.unwrap_or(0.0);
            format!(
                "Forecast crest {s:.1} ft {} — {}{issued}",
                when(r.time, tz),
                cat_label(t.category(s))
            )
        }
        Some((r, PeakKind::AtEnd)) => {
            let s = r.stage_ft.unwrap_or(0.0);
            format!(
                "Forecast to keep rising, to {s:.1} ft by {} — {}{issued}",
                when(r.time, tz),
                cat_label(t.category(s))
            )
        }
        _ => {
            let end = ahead.last().and_then(|r| r.stage_ft.map(|s| (r.time, s)));
            match end {
                Some((tm, s)) => {
                    format!("Forecast to fall, to {s:.1} ft by {}{issued}", when(tm, tz))
                }
                None => format!("Forecast to fall{issued}"),
            }
        }
    };
    ui.label(RichText::new(text).size(11.5));
}

fn stages_and_impacts(ui: &mut egui::Ui, d: &GaugeDetail, h: Option<&Hydrograph>) {
    let levels = d.thresholds.levels();
    if levels.is_empty() {
        ui.label(
            RichText::new("No flood stages are defined for this gauge.")
                .size(11.0)
                .weak(),
        );
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Flood stages:").size(11.0));
            for (c, s) in levels.iter().rev() {
                chip(ui, &format!("{} {s:.0} ft", stage_name(*c)), cat_color(*c));
            }
        });
    }
    // What happens at this level now, and at the forecast crest if that is higher.
    let now_stage = h
        .and_then(|h| h.observed.latest())
        .and_then(|r| r.stage_ft)
        .or(d.summary.stage_ft);
    let fc_stage = h
        .and_then(|h| {
            h.forecast
                .readings
                .iter()
                .filter_map(|r| r.stage_ft)
                .max_by(f64::total_cmp)
        })
        .or(d.summary.forecast_ft);
    let now_impact = now_stage.and_then(|s| d.impact_at(s));
    if let Some(i) = now_impact {
        impact_label(ui, "At the current level", i);
    }
    if let Some(i) = fc_stage.and_then(|s| d.impact_at(s)) {
        if now_impact.is_none_or(|n| n.stage_ft < i.stage_ft) {
            impact_label(ui, "At the forecast crest", i);
        }
    }
    if !d.impacts.is_empty() {
        egui::CollapsingHeader::new(format!("All impacts ({})", d.impacts.len()))
            .id_salt(("gauge-impacts", &d.summary.lid))
            .show(ui, |ui| {
                for i in &d.impacts {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("{:.1} ft", i.stage_ft))
                                .strong()
                                .color(cat_color(d.thresholds.category(i.stage_ft))),
                        );
                        ui.label(RichText::new(&i.statement).size(11.0));
                    });
                }
            });
    }
}

fn impact_label(ui: &mut egui::Ui, heading: &str, i: &wxdata::river::Impact) {
    ui.label(
        RichText::new(format!("{heading} ({:.1} ft):", i.stage_ft))
            .size(11.0)
            .strong(),
    );
    ui.label(RichText::new(&i.statement).size(11.0));
}

fn crest_history(ui: &mut egui::Ui, d: &GaugeDetail, tz: Option<Tz>) {
    if d.historic.is_empty() && d.recent.is_empty() {
        return;
    }
    if let Some(rec) = d.record() {
        ui.label(
            RichText::new(format!(
                "Record crest {:.2} ft on {}",
                rec.stage_ft,
                rec.time.format("%b %-d, %Y")
            ))
            .size(11.0),
        );
    }
    egui::CollapsingHeader::new("Crest history")
        .id_salt(("gauge-crests", &d.summary.lid))
        .show(ui, |ui| {
            let row = |ui: &mut egui::Ui, c: &wxdata::river::Crest| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{:>6.2} ft", c.stage_ft))
                            .monospace()
                            .color(cat_color(d.thresholds.category(c.stage_ft))),
                    );
                    // Old crests have no time of day worth showing.
                    let date = match tz {
                        Some(z) => c.time.with_timezone(&z).format("%b %-d, %Y").to_string(),
                        None => c.time.format("%b %-d, %Y").to_string(),
                    };
                    ui.label(RichText::new(date).size(11.0));
                    if let Some(f) = c.flow_cfs {
                        ui.label(RichText::new(flow_text(f, "cfs")).size(10.5).weak());
                    }
                    if c.preliminary {
                        ui.label(RichText::new("preliminary").size(10.0).weak());
                    }
                });
            };
            if !d.recent.is_empty() {
                ui.label(RichText::new("Recent").size(11.0).strong());
                for c in d.recent.iter().take(5) {
                    row(ui, c);
                }
            }
            if !d.historic.is_empty() {
                ui.label(RichText::new("Highest on record").size(11.0).strong());
                for c in d.historic.iter().take(10) {
                    row(ui, c);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: i64) -> DateTime<Utc> {
        "2026-09-25T00:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::hours(h)
    }

    fn r(h: i64, s: f64) -> Reading {
        Reading {
            time: at(h),
            stage_ft: Some(s),
            flow: None,
        }
    }

    #[test]
    fn the_forecast_starts_after_the_newest_observation() {
        let h = Hydrograph {
            observed: Series {
                readings: vec![r(-30, 9.0), r(-2, 10.0), r(0, 10.5)],
                ..Default::default()
            },
            forecast: Series {
                readings: vec![r(-6, 9.8), r(0, 10.4), r(6, 12.0), r(12, 11.0)],
                ..Default::default()
            },
        };
        let (obs, fc) = visible(&h, Past::Day, true, at(0));
        assert_eq!(obs.len(), 2, "the 30-hour-old reading is outside a day");
        assert_eq!(fc.len(), 2, "forecast hours already observed are dropped");
        assert_eq!(fc[0].time, at(6));
        // A day of history shows two days of forecast, not the whole of a long one.
        let mut long = h.clone();
        long.forecast.readings.push(r(47, 11.5));
        long.forecast.readings.push(r(60, 11.0));
        assert_eq!(visible(&long, Past::Day, true, at(0)).1.len(), 3);
        assert_eq!(visible(&long, Past::ThreeDays, true, at(0)).1.len(), 4);
        let (_, none) = visible(&h, Past::Week, false, at(0));
        assert!(none.is_empty());
    }

    #[test]
    fn the_stage_range_reaches_up_to_the_next_flood_stage() {
        let t = Thresholds {
            action: Some(25.0),
            minor: Some(33.0),
            moderate: None,
            major: None,
        };
        let (lo, hi) = stage_range(&[12.0, 13.0], &t, true).unwrap();
        assert!(lo < 12.0 && hi > 25.0 && hi < 33.0, "{lo} {hi}");
        let (_, hi) = stage_range(&[12.0, 13.0], &t, false).unwrap();
        assert!(hi < 15.0, "without stages the data fills the graph: {hi}");
        assert!(stage_range(&[], &t, true).is_none());
        // A flat line still gets a foot of room.
        let (lo, hi) = stage_range(&[5.0], &Thresholds::default(), true).unwrap();
        assert!(hi - lo >= 1.0);
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(flow_text(0.234, "kcfs"), "234 cfs");
        assert_eq!(flow_text(34.4, "kcfs"), "34.4 kcfs");
        assert_eq!(flow_text(550_000.0, "cfs"), "550.0 kcfs");
        assert_eq!(season_label("SON"), "Sep–Nov");
        assert_eq!(season_label("NDJ"), "Nov–Jan");
        assert_eq!(season_label("JJA"), "Jun–Aug");
        assert_eq!(season_label("xx"), "xx");
        assert_eq!(nice_step(13.0, 5.0), 5.0);
        assert_eq!(nice_step(0.4, 5.0), 0.1);
    }
}
