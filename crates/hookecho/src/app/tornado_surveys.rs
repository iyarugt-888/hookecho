//! Historical NWS tornado surveys on the map: the tornado tool, rebuilt around the Damage
//! Assessment Toolkit (`wxdata::dat`). Pick a day (or one of the notable events) and every
//! survey of that convective day is drawn: each path coloured by its rating, each damage
//! indicator the crews rated as a dot. Pick an event to frame its path, read its length, width,
//! peak wind and casualties, step through its indicators strongest first, and jump the radar to
//! that time. Or click the map with the tool for the surveyed paths near that spot, beside the
//! SPC record and the warning history the old climatology window showed.

use super::*;
use wxdata::dat::{DamagePoint, SurveyEvent};

/// How far from a clicked spot the surveyed paths are listed.
const NEAR_KM: f64 = 40.0;
/// The earliest storm date asked for near a point: the DAT holds little before this.
const NEAR_SINCE: (i32, u32, u32) = (2007, 1, 1);
/// Indicators listed for the picked event; the map draws them all.
const LISTED: usize = 300;

/// Days people come back to, each checked to have surveys in the DAT (Joplin 2011 has ten
/// indicators and no path there, so it is not offered).
pub(crate) const NOTABLE: &[(&str, (i32, u32, u32))] = &[
    ("Moore, OK (EF5)", (2013, 5, 20)),
    ("El Reno, OK (widest on record)", (2013, 5, 31)),
    ("Lee County, AL (EF4)", (2019, 3, 3)),
    ("27 April 2011 outbreak", (2011, 4, 27)),
    ("Washington, IL (EF4)", (2013, 11, 17)),
    ("Vilonia, AR (EF4)", (2014, 4, 27)),
    ("Nashville, TN (EF3)", (2020, 3, 2)),
    ("Mayfield, KY (EF4)", (2021, 12, 10)),
    ("Rolling Fork, MS (EF4)", (2023, 3, 24)),
    ("Greenfield, IA (EF4)", (2024, 5, 21)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SurveyMode {
    #[default]
    Day,
    Near,
}

enum Reply {
    Events(u64, Result<Vec<SurveyEvent>, String>),
    /// A near-point path's own indicators, for the event at this index.
    Event(u64, usize, Result<SurveyEvent, String>),
}

pub(crate) struct Surveys {
    pub mode: SurveyMode,
    /// Which tab the shown events were read for: each tab shows only its own.
    events_for: SurveyMode,
    pub day: chrono::NaiveDate,
    pub day_text: String,
    pub events: Vec<SurveyEvent>,
    pub selected: Option<usize>,
    /// What is being fetched, while it is.
    pub loading: Option<String>,
    pub error: Option<String>,
    /// Paths below this rating are hidden from the list and the map.
    pub min_ef: u8,
    pub near: Option<(f64, f64)>,
    /// What the shown events are for, in words.
    pub title: String,
    generation: u64,
    rx: Option<std::sync::mpsc::Receiver<Reply>>,
}

impl Default for Surveys {
    fn default() -> Self {
        let day = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).expect("a real date");
        Surveys {
            mode: SurveyMode::Day,
            events_for: SurveyMode::Day,
            day,
            day_text: day.to_string(),
            events: Vec::new(),
            selected: None,
            loading: None,
            error: None,
            min_ef: 0,
            near: None,
            title: String::new(),
            generation: 0,
            rx: None,
        }
    }
}

impl Surveys {
    /// Whether an event passes the rating filter. Unrated damage shows only with "All".
    pub(crate) fn shown(&self, e: &SurveyEvent) -> bool {
        self.min_ef == 0 || e.rating().is_some_and(|r| r >= self.min_ef)
    }

    fn begin(&mut self, what: String) -> (u64, std::sync::mpsc::Sender<Reply>) {
        self.generation += 1;
        self.loading = Some(what);
        self.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        (self.generation, tx)
    }
}

/// The time zone a survey reads in: its nearest radar's.
fn local_tz(lon: f64, lat: f64) -> Option<wxdata::tz::Tz> {
    crate::geo::nearest_site_id(lon, lat).and_then(|id| wxdata::tz::site_tz(&id))
}

/// "2:04 PM CDT", in the event's own zone, or UTC.
fn clock(e: &SurveyEvent) -> String {
    let Some(t) = e.time() else {
        return "time not recorded".into();
    };
    let at = e
        .bounds()
        .map(|(w, s, east, n)| ((w + east) / 2.0, (s + n) / 2.0));
    match at.and_then(|(lon, lat)| local_tz(lon, lat)) {
        Some(tz) => t.with_timezone(&tz).format("%-I:%M %p %Z").to_string(),
        None => t.format("%H:%M UTC").to_string(),
    }
}

fn rating_text(e: &SurveyEvent) -> String {
    e.rating()
        .map_or_else(|| "EF?".to_string(), |r| format!("EF{r}"))
}

/// "17.5 mi · 1,900 yd · 210 mph · 24 deaths", from what the survey recorded.
fn facts(e: &SurveyEvent) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &e.track {
        if let Some(l) = t.length_mi.filter(|l| *l > 0.0) {
            parts.push(format!("{l:.1} mi"));
        }
        if let Some(w) = t.width_yd.filter(|w| *w > 0.0) {
            parts.push(format!("{w:.0} yd wide"));
        }
        if let Some(w) = t.max_wind {
            parts.push(format!("{w} mph"));
        }
    } else {
        parts.push("no surveyed path".into());
    }
    let deaths = e.deaths();
    if deaths > 0 {
        parts.push(format!("{deaths} killed"));
    }
    if !e.points.is_empty() {
        parts.push(format!("{} indicators", e.points.len()));
    }
    parts.join(" \u{b7} ")
}

/// How many indicators carry each rating, EF0 to EF5, then unrated.
fn ef_counts(points: &[DamagePoint]) -> [usize; 7] {
    let mut n = [0; 7];
    for p in points {
        n[wxdata::dat::ef_number(&p.efscale).map_or(6, usize::from)] += 1;
    }
    n
}

impl HookEchoApp {
    /// Load every survey of the convective day `day`.
    pub(crate) fn load_survey_day(&mut self, day: chrono::NaiveDate) {
        let s = &mut self.surveys;
        s.mode = SurveyMode::Day;
        s.events_for = SurveyMode::Day;
        s.day = day;
        s.day_text = day.to_string();
        s.selected = None;
        s.events.clear();
        s.title = format!("{} (12 UTC to 12 UTC)", day.format("%A %-d %B %Y"));
        let (generation, tx) = s.begin(format!("Reading the surveys for {day}\u{2026}"));
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let got = wxdata::dat::fetch_day(&http, day)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Reply::Events(generation, got));
        });
    }

    /// List the surveyed paths near a clicked spot (the tool's click).
    pub(crate) fn query_surveys_near(&mut self, lon: f64, lat: f64) {
        let s = &mut self.surveys;
        s.mode = SurveyMode::Near;
        s.events_for = SurveyMode::Near;
        s.near = Some((lon, lat));
        s.selected = None;
        s.events.clear();
        s.title = format!("Within 25 mi of {lat:.3}, {lon:.3}");
        let (generation, tx) = s.begin("Reading the surveyed paths near here\u{2026}".into());
        let http = self.http.clone();
        let since = chrono::NaiveDate::from_ymd_opt(NEAR_SINCE.0, NEAR_SINCE.1, NEAR_SINCE.2)
            .expect("a real date")
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc();
        self.spawner.spawn(async move {
            let got = wxdata::dat::fetch_paths_near(&http, lon, lat, NEAR_KM, since, Utc::now())
                .await
                .map(|tracks| {
                    tracks
                        .into_iter()
                        .map(|t| SurveyEvent {
                            track: Some(t),
                            points: Vec::new(),
                        })
                        .collect()
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Reply::Events(generation, got));
        });
    }

    /// Pick an event: frame it, and read its indicators if they are not here yet.
    pub(crate) fn select_survey(&mut self, i: usize) {
        let Some(e) = self.surveys.events.get(i) else {
            return;
        };
        let bounds = e.bounds();
        let unread = e.track.clone().filter(|_| e.points.is_empty());
        self.surveys.selected = Some(i);
        if let Some(b) = bounds {
            let pad = 0.02;
            self.fit_view((b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad));
        }
        let Some(track) = unread else {
            return;
        };
        let generation = self.surveys.generation;
        self.surveys.loading = Some("Reading this path's damage indicators\u{2026}".into());
        let (tx, rx) = std::sync::mpsc::channel();
        self.surveys.rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let got = wxdata::dat::fetch_event(&http, track)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Reply::Event(generation, i, got));
        });
    }

    /// Jump the active pane's radar to the event: its nearest radar, at its time, framed on it.
    pub(crate) fn survey_radar(&mut self, i: usize) {
        let Some(e) = self.surveys.events.get(i) else {
            return;
        };
        let (Some(b), Some(t)) = (e.bounds(), e.time()) else {
            return;
        };
        let (lon, lat) = match &e.track {
            Some(t) => (t.path[0][0], t.path[0][1]),
            None => ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0),
        };
        let Some(site) = crate::geo::nearest_site_id(lon, lat) else {
            return;
        };
        let zoom = self.views[self.active].camera.zoom;
        self.goto_view(&site, lon, lat, zoom, Some(t));
        let pad = 0.02;
        self.fit_view((b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad));
    }

    /// Take what the fetches sent.
    pub(crate) fn sync_surveys(&mut self) {
        let Some(rx) = &self.surveys.rx else {
            return;
        };
        let Ok(reply) = rx.try_recv() else {
            return;
        };
        let s = &mut self.surveys;
        s.rx = None;
        s.loading = None;
        match reply {
            Reply::Events(g, got) if g == s.generation => match got {
                Ok(events) => s.events = events,
                Err(e) => s.error = Some(e),
            },
            Reply::Event(g, i, got) if g == s.generation => match got {
                Ok(event) => {
                    if let Some(slot) = s.events.get_mut(i) {
                        *slot = event;
                    }
                }
                Err(e) => s.error = Some(e),
            },
            _ => {}
        }
    }

    /// The tool's window body: the day or the spot, the events, and the picked one.
    pub(crate) fn surveys_ui(&mut self, ui: &mut egui::Ui) {
        use egui::RichText;
        self.sync_surveys();
        let mut load_day = None;
        let mut select = None;
        let mut radar = None;
        let mut open_point = None;
        let s = &mut self.surveys;
        let mut near_again = None;
        ui.horizontal(|ui| {
            let day = ui.selectable_value(&mut s.mode, SurveyMode::Day, "By day");
            let near = ui.selectable_value(&mut s.mode, SurveyMode::Near, "Near a point");
            // A tab shows its own results: the day reloads, the last spot is asked again.
            if day.clicked() && s.events_for != SurveyMode::Day {
                load_day = Some(s.day);
            }
            if near.clicked() && s.events_for != SurveyMode::Near {
                near_again = s.near;
                if s.near.is_none() {
                    s.events.clear();
                    s.selected = None;
                    s.title.clear();
                    s.events_for = SurveyMode::Near;
                }
            }
        });
        if s.mode == SurveyMode::Day {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("\u{25c0}")
                    .on_hover_text("The day before")
                    .clicked()
                {
                    load_day = s.day.pred_opt();
                }
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut s.day_text)
                        .desired_width(96.0)
                        .hint_text("YYYY-MM-DD"),
                );
                if ui
                    .button("\u{25b6}")
                    .on_hover_text("The day after")
                    .clicked()
                {
                    load_day = s.day.succ_opt();
                }
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Show").clicked() || enter {
                    match chrono::NaiveDate::parse_from_str(s.day_text.trim(), "%Y-%m-%d") {
                        Ok(d) => load_day = Some(d),
                        Err(_) => s.error = Some("Write the day as YYYY-MM-DD".into()),
                    }
                }
                egui::ComboBox::from_id_salt("survey_notable")
                    .selected_text("Notable days")
                    .show_ui(ui, |ui| {
                        for (name, (y, m, d)) in NOTABLE {
                            let day = chrono::NaiveDate::from_ymd_opt(*y, *m, *d);
                            if ui
                                .selectable_label(day == Some(s.day), format!("{name} \u{b7} {y}"))
                                .clicked()
                            {
                                load_day = day;
                            }
                        }
                    });
            });
        } else if s.near.is_none() {
            ui.weak("Click the map with the tornado tool to list the surveyed paths near a spot.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Show");
            for (ef, label) in [
                (0, "All"),
                (1, "EF1+"),
                (2, "EF2+"),
                (3, "EF3+"),
                (4, "EF4+"),
            ] {
                ui.selectable_value(&mut s.min_ef, ef, label);
            }
        });
        if let Some(what) = &s.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(what.as_str());
            });
        }
        if let Some(e) = &s.error {
            ui.colored_label(egui::Color32::from_rgb(230, 110, 90), e.as_str());
        }
        if !s.title.is_empty() {
            ui.strong(s.title.as_str());
        }
        let shown: Vec<usize> = (0..s.events.len())
            .filter(|&i| s.shown(&s.events[i]))
            .collect();
        if s.loading.is_none() && !s.title.is_empty() {
            let paths = shown
                .iter()
                .filter(|&&i| s.events[i].track.is_some())
                .count();
            let points: usize = shown.iter().map(|&i| s.events[i].points.len()).sum();
            let strongest = shown.iter().filter_map(|&i| s.events[i].rating()).max();
            ui.weak(if shown.is_empty() {
                "No surveys here. The DAT holds NWS surveys from about 2007, unevenly before 2012."
                    .to_string()
            } else {
                format!(
                    "{} surveyed paths{}{}",
                    paths,
                    if points > 0 {
                        format!(", {points} damage indicators")
                    } else {
                        String::new()
                    },
                    strongest.map_or(String::new(), |r| format!(" \u{b7} strongest rated EF{r}"))
                )
            });
        }
        egui::ScrollArea::vertical()
            .id_salt("survey_events")
            .max_height(200.0)
            .show(ui, |ui| {
                for &i in &shown {
                    let e = &s.events[i];
                    let when = if s.mode == SurveyMode::Near {
                        e.time()
                            .map_or_else(String::new, |t| t.format("%Y-%m-%d ").to_string())
                    } else {
                        String::new()
                    };
                    let color = super::ef_color(&rating_text(e));
                    let text = RichText::new(format!(
                        "{}  {when}{}  {}  {}",
                        rating_text(e),
                        clock(e),
                        e.office(),
                        facts(e)
                    ))
                    .color(color);
                    if ui.selectable_label(s.selected == Some(i), text).clicked() {
                        select = Some(i);
                    }
                }
            });
        if let Some(i) = s.selected.filter(|i| *i < s.events.len()) {
            let e = &s.events[i];
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} tornado \u{b7} NWS {} \u{b7} {}",
                    rating_text(e),
                    e.office(),
                    e.time().map_or_else(String::new, |t| format!(
                        "{} {}",
                        t.format("%Y-%m-%d"),
                        clock(e)
                    ))
                ))
                .strong()
                .color(super::ef_color(&rating_text(e))),
            );
            ui.horizontal_wrapped(|ui| {
                if let Some(t) = &e.track {
                    if let Some(l) = t.length_mi.filter(|l| *l > 0.0) {
                        crate::theme::stat_card(ui, "Length", &format!("{l:.1} mi"));
                    }
                    if let Some(w) = t.width_yd.filter(|w| *w > 0.0) {
                        crate::theme::stat_card(ui, "Width", &format!("{w:.0} yd"));
                    }
                    if let Some(w) = t.max_wind {
                        crate::theme::stat_card(ui, "Peak wind", &format!("{w} mph"));
                    }
                    // Only what the path record states: a re-analysed path can carry no
                    // casualties at all (Moore's reads 0 of the 24 killed).
                    if t.fatalities > 0 {
                        crate::theme::stat_card(ui, "Killed", &t.fatalities.to_string());
                    }
                    if t.injuries > 0 {
                        crate::theme::stat_card(ui, "Injured", &t.injuries.to_string());
                    }
                }
                crate::theme::stat_card(ui, "Indicators", &e.points.len().to_string());
            });
            if !e.points.is_empty() {
                let n = ef_counts(&e.points);
                ui.horizontal_wrapped(|ui| {
                    for (k, label) in ["EF0", "EF1", "EF2", "EF3", "EF4", "EF5", "unrated"]
                        .iter()
                        .enumerate()
                    {
                        if n[k] > 0 {
                            ui.label(
                                RichText::new(format!("{label} {}", n[k]))
                                    .color(super::ef_color(label)),
                            );
                        }
                    }
                });
            }
            if let Some(c) = e.track.as_ref().and_then(|t| t.comments.as_ref()) {
                ui.weak(c.as_str());
            }
            ui.horizontal(|ui| {
                if ui.button("Zoom to path").clicked() {
                    select = Some(i);
                }
                if ui
                    .button("Radar at this time")
                    .on_hover_text("The nearest radar's archive at the survey's storm time")
                    .clicked()
                {
                    radar = Some(i);
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("survey_points")
                .max_height(220.0)
                .show(ui, |ui| {
                    for p in e.points.iter().take(LISTED) {
                        let mut line = format!(
                            "{}  {}",
                            if p.efscale.is_empty() {
                                "EF?"
                            } else {
                                &p.efscale
                            },
                            if p.damage.is_empty() {
                                "Damage"
                            } else {
                                &p.damage
                            }
                        );
                        if !p.dod.is_empty() {
                            line.push_str(&format!(" \u{b7} {}", p.dod));
                        }
                        if let Some(w) = p.windspeed {
                            line.push_str(&format!(" \u{b7} {w} mph"));
                        }
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(line).color(super::ef_color(&p.efscale)),
                            )
                            .clicked()
                        {
                            open_point = Some(p.clone());
                        }
                    }
                    if e.points.len() > LISTED {
                        ui.weak(format!(
                            "\u{2026} and {} weaker ones, on the map",
                            e.points.len() - LISTED
                        ));
                    }
                });
        }
        ui.weak("Source: NWS Damage Assessment Toolkit surveys. Ratings are the surveyors'.");
        if let Some(d) = load_day {
            self.load_survey_day(d);
        }
        if let Some((lon, lat)) = near_again {
            self.query_surveys_near(lon, lat);
        }
        if let Some(i) = select {
            self.select_survey(i);
        }
        if let Some(i) = radar {
            self.survey_radar(i);
        }
        if let Some(p) = open_point {
            let zoom = self.views[self.active].camera.zoom.max(12.0);
            self.views[self.active].camera =
                crate::render::mercator::Camera::at_lonlat(p.lon, p.lat, zoom);
            self.open_damage_point(&p, ui.ctx());
        }
    }

    /// Draw the shown events: paths coloured and widened by rating, the picked one outlined, and
    /// the damage indicators (all of the picked event's, the others' once zoomed in).
    pub(crate) fn paint_surveys(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        let s = &self.surveys;
        if s.events_for != s.mode {
            return;
        }
        let to_screen = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        let order = (0..s.events.len())
            .filter(|i| Some(*i) != s.selected)
            .chain(s.selected);
        for i in order {
            let e = &s.events[i];
            if !s.shown(e) {
                continue;
            }
            let picked = s.selected == Some(i);
            let color = super::ef_color(&rating_text(e));
            if let Some(t) = &e.track {
                let width = 2.0 + f32::from(e.rating().unwrap_or(0)) * 0.6;
                for run in t.drawable_runs() {
                    let pts: Vec<egui::Pos2> = run.iter().map(|p| to_screen(p[0], p[1])).collect();
                    if picked {
                        painter.add(egui::Shape::line(
                            pts.clone(),
                            egui::Stroke::new(width + 3.0, egui::Color32::WHITE),
                        ));
                    }
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
                }
                let start = t.path.first().map(|p| to_screen(p[0], p[1]));
                if let Some(start) = start.filter(|p| prect.contains(*p)) {
                    painter.circle_filled(start, width * 0.8, color);
                    // Labelled when picked, or for the strong ones once zoomed in: every EF0's
                    // label at once is a wall of text.
                    if picked || (cam.zoom >= 8.0 && e.rating().is_some_and(|r| r >= 3)) {
                        painter.text(
                            start + egui::vec2(6.0, -6.0),
                            egui::Align2::LEFT_BOTTOM,
                            rating_text(e),
                            egui::FontId::proportional(12.0),
                            egui::Color32::WHITE,
                        );
                    }
                }
            }
            if !picked && cam.zoom < 9.0 {
                continue;
            }
            // Weakest first, so the strongest damage sits on top.
            let r = if picked { 4.0 } else { 2.5 };
            for p in e.points.iter().rev() {
                let at = to_screen(p.lon, p.lat);
                if !prect.contains(at) {
                    continue;
                }
                painter.circle_filled(at, r, super::ef_color(&p.efscale));
                painter.circle_stroke(
                    at,
                    r,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                );
            }
        }
        if let (SurveyMode::Near, Some((lon, lat))) = (s.mode, s.near) {
            painter.circle_stroke(
                to_screen(lon, lat),
                6.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
        }
    }

    /// What a click on the map hits among the drawn surveys: a damage indicator (the picked
    /// event's first), else a path, by index.
    pub(crate) fn survey_hit(
        &self,
        pos: egui::Pos2,
        prect: egui::Rect,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
        r2: f32,
    ) -> Option<Result<DamagePoint, usize>> {
        let s = &self.surveys;
        let to_screen = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        let near = |at: egui::Pos2| (at - pos).length_sq() <= r2;
        if s.events_for != s.mode {
            return None;
        }
        let pickable =
            |i: usize| s.shown(&s.events[i]) && (s.selected == Some(i) || cam.zoom >= 9.0);
        for i in s.selected.into_iter().chain(0..s.events.len()) {
            if i >= s.events.len() || !pickable(i) {
                continue;
            }
            if let Some(p) = s.events[i]
                .points
                .iter()
                .find(|p| near(to_screen(p.lon, p.lat)))
            {
                return Some(Ok(p.clone()));
            }
        }
        (0..s.events.len())
            .filter(|&i| s.shown(&s.events[i]))
            .find(|&i| {
                s.events[i].track.as_ref().is_some_and(|t| {
                    t.path.windows(2).any(|w| {
                        let (a, b) = (to_screen(w[0][0], w[0][1]), to_screen(w[1][0], w[1][1]));
                        let ab = b - a;
                        let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        near(a + ab * t)
                    })
                })
            })
            .map(Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ef: &str) -> DamagePoint {
        DamagePoint {
            lon: -97.5,
            lat: 35.3,
            efscale: ef.into(),
            damage: String::new(),
            dod: String::new(),
            windspeed: None,
            deaths: 1,
            injuries: 0,
            office: "OUN".into(),
            comments: None,
            image: None,
            storm: None,
            path_id: None,
        }
    }

    #[test]
    fn the_filter_and_the_counts_follow_the_ratings() {
        let mut s = Surveys::default();
        let weak = SurveyEvent {
            track: None,
            points: vec![point("EF0"), point("EFU")],
        };
        let strong = SurveyEvent {
            track: None,
            points: vec![point("EF4"), point("EF2")],
        };
        assert!(s.shown(&weak) && s.shown(&strong));
        s.min_ef = 3;
        assert!(!s.shown(&weak) && s.shown(&strong));
        assert_eq!(ef_counts(&weak.points), [1, 0, 0, 0, 0, 0, 1]);
        assert_eq!(rating_text(&strong), "EF4");
        assert!(facts(&strong).contains("no surveyed path"));
        assert!(facts(&strong).contains("2 killed"), "{}", facts(&strong));
    }

    #[test]
    fn every_notable_day_is_a_real_date() {
        for (name, (y, m, d)) in NOTABLE {
            assert!(
                chrono::NaiveDate::from_ymd_opt(*y, *m, *d).is_some(),
                "{name}"
            );
        }
    }

    /// Every notable day has surveys (paths or rated indicators), live.
    /// `cargo test -p hookecho --lib notable_days_have_surveys_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network (NWS DAT)"]
    async fn notable_days_have_surveys_live() {
        let http = reqwest::Client::new();
        for (name, (y, m, d)) in NOTABLE {
            let day = chrono::NaiveDate::from_ymd_opt(*y, *m, *d).unwrap();
            let events = wxdata::dat::fetch_day(&http, day).await.unwrap();
            let paths = events.iter().filter(|e| e.track.is_some()).count();
            let points: usize = events.iter().map(|e| e.points.len()).sum();
            let top = events.iter().filter_map(|e| e.rating()).max();
            println!("{name} {day}: {paths} paths, {points} indicators, strongest EF{top:?}");
            // A day can hold rated indicators with no drawn path (El Reno, 2013).
            assert!(paths + points > 0, "{name}: no surveys");
        }
    }
}
