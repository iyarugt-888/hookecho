//! Tropical model guidance on the map: a track per model ("spaghetti"), the observed best track,
//! and markers for the invests NHC is running models on but not yet advising.
//!
//! The data is `wxdata::atcf`: every active system's a-deck (the latest run of each model) and
//! b-deck (the best track). What is drawn is chosen per model group and per model in the
//! Tropical window's Models tab ([`crate::ui::tropical_window`]); this module holds that choice
//! and paints it.
//!
//! Draw order is least to most important: ensemble members (thin, faint) under the deterministic
//! models, consensus and the official track on top, then the best track, which is fact rather
//! than forecast. Each line ends in its model id; a dot marks every 24 hours.

use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::collections::HashSet;
use wxdata::atcf::{unwrap_lon, Aid, Group, Guidance};
use wxdata::clock::Instant;
use wxdata::tz::Tz;

/// Refetch cadence. The a-decks grow with each model cycle (early aids about an hour after
/// synoptic time, late models some hours later), so half-hourly catches each within the half
/// hour without re-pulling a megabyte-scale file every few minutes.
pub const REFRESH_SECS: u64 = 30 * 60;

/// What the model-guidance layer shows, and the guidance itself.
pub struct Spaghetti {
    /// Model tracks on the map (and the fetch behind them).
    pub enabled: bool,
    /// The observed track behind each system.
    pub best_track: bool,
    /// The early ("interpolated") aids alongside the late models they are made from.
    pub interpolated: bool,
    /// Model ids at the ends of the lines.
    pub labels: bool,
    /// Which groups draw, indexed like [`Group::ALL`].
    pub groups: [bool; Group::ALL.len()],
    /// Individual models switched off within a group that is on.
    pub hidden: HashSet<String>,
    /// Draw only this system's guidance (`None`: every system).
    pub focus: Option<String>,
    pub guidance: Vec<Guidance>,
    pub last_fetch: Option<Instant>,
    pub loading: bool,
    pub error: Option<String>,
    /// Set by a surface that wants the Tropical window opened on its Models tab (the layer
    /// options); the app opens it and clears this.
    pub open_window: bool,
    rx: Option<std::sync::mpsc::Receiver<Result<Vec<Guidance>, String>>>,
}

impl Default for Spaghetti {
    fn default() -> Self {
        Self {
            // Off until asked for: every active system's a-deck is a few hundred kilobytes
            // compressed, and in a busy season that is megabytes each half hour.
            enabled: false,
            best_track: true,
            interpolated: false,
            labels: true,
            groups: Group::ALL.map(Group::default_on),
            hidden: HashSet::new(),
            focus: None,
            guidance: Vec::new(),
            last_fetch: None,
            loading: false,
            error: None,
            open_window: false,
            rx: None,
        }
    }
}

pub fn group_index(g: Group) -> usize {
    Group::ALL.iter().position(|x| *x == g).unwrap_or(0)
}

impl Spaghetti {
    pub fn group_on(&self, g: Group) -> bool {
        self.groups[group_index(g)]
    }

    /// Whether a run is drawn: its group is on, it is not hidden, and it is not an interpolated
    /// aid while those are off. Intensity-only aids never draw as tracks.
    pub fn shows(&self, a: &Aid) -> bool {
        let i = a.info();
        i.group != Group::Other
            && self.group_on(i.group)
            && (self.interpolated || !i.interpolated)
            && !self.hidden.contains(&a.tech)
    }

    /// The systems in view of the focus.
    pub fn systems(&self) -> impl Iterator<Item = &Guidance> {
        self.guidance
            .iter()
            .filter(|g| self.focus.as_ref().is_none_or(|f| *f == g.id))
    }

    pub fn find(&self, id: &str) -> Option<&Guidance> {
        self.guidance.iter().find(|g| g.id == id)
    }

    /// Land a fetch.
    pub fn ingest(&mut self, result: Result<Vec<Guidance>, String>) {
        self.loading = false;
        match result {
            Ok(g) => {
                self.guidance = g;
                self.error = None;
                // A focused system that has dissipated leaves nothing drawn and no way to tell.
                if let Some(f) = &self.focus {
                    if self.find(f).is_none() {
                        self.focus = None;
                    }
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Start a fetch of every active system's guidance.
    pub fn fetch(
        &mut self,
        spawner: &crate::rt::Spawner,
        http: &reqwest::Client,
        ctx: &egui::Context,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.loading = true;
        self.last_fetch = Some(Instant::now());
        let (http, ctx) = (http.clone(), ctx.clone());
        spawner.spawn(async move {
            let r = wxdata::atcf::fetch_active(&http)
                .await
                .map_err(|e| format!("model guidance: {e}"));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
    }

    /// Land a finished fetch, if one has.
    pub fn poll(&mut self) {
        let Some(r) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.rx = None;
        self.ingest(r);
    }

    /// True when the fetch clock says to refetch.
    pub fn due(&self) -> bool {
        self.enabled
            && !self.loading
            && self
                .last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= REFRESH_SECS)
    }

    /// The invest (or other system with a best track) whose marker is under `pos`.
    pub fn hit(
        &self,
        pos: Pos2,
        radius2: f32,
        to_screen: impl Fn(f64, f64) -> Pos2,
    ) -> Option<String> {
        if !self.enabled {
            return None;
        }
        self.guidance
            .iter()
            .filter(|g| g.is_invest())
            .filter_map(|g| {
                let f = g.latest_fix()?;
                let d2 = to_screen(f.lon, f.lat).distance_sq(pos);
                (d2 <= radius2).then_some((g.id.clone(), d2))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}

/// A run's line width and color: members thin and faint, aids that blend many models (and the
/// official forecast) heavier.
fn style(a: &Aid) -> (f32, Color32) {
    let i = a.info();
    let c = Color32::from_rgb(i.rgb[0], i.rgb[1], i.rgb[2]);
    match i.group {
        Group::EnsembleMember => (1.0, c.gamma_multiply(0.55)),
        Group::Official => (2.6, c),
        Group::Consensus | Group::EnsembleMean => (2.2, c),
        Group::Statistical => (1.2, c.gamma_multiply(0.8)),
        _ => (1.7, c),
    }
}

/// Draw order: members first, the official forecast last.
fn rank(g: Group) -> u8 {
    match g {
        Group::EnsembleMember => 0,
        Group::Statistical => 1,
        Group::Other => 1,
        Group::Global | Group::Hurricane => 2,
        Group::EnsembleMean => 3,
        Group::Consensus => 4,
        Group::Official => 5,
    }
}

/// `12Z Sep 25` / local.
fn cycle_label(t: DateTime<Utc>) -> String {
    t.format("%HZ %b %-d").to_string()
}

fn valid_label(t: DateTime<Utc>, tz: Option<Tz>) -> String {
    match tz {
        Some(z) => t.with_timezone(&z).format("%a %-I %p %Z").to_string(),
        None => t.format("%a %HZ").to_string(),
    }
}

/// The screen positions of a run, with longitudes unwrapped so a track crossing the dateline
/// stays one line.
fn project(
    points: impl Iterator<Item = (f64, f64)>,
    to_screen: &impl Fn(f64, f64) -> Pos2,
) -> Vec<Pos2> {
    let mut prev: Option<f64> = None;
    points
        .map(|(lat, lon)| {
            let l = prev.map_or(lon, |p| unwrap_lon(p, lon));
            prev = Some(l);
            to_screen(l, lat)
        })
        .collect()
}

/// Paint the guidance. Returns the hover readout for the point under `hover`, if any.
pub fn draw(
    painter: &Painter,
    sp: &Spaghetti,
    clip: Rect,
    zoom: f32,
    to_screen: impl Fn(f64, f64) -> Pos2,
    hover: Option<Pos2>,
    tz: Option<Tz>,
) -> Option<String> {
    if !sp.enabled {
        return None;
    }
    let mut best_hit: Option<(f32, String)> = None;
    let mut consider = |p: Pos2, text: &dyn Fn() -> String| {
        if let Some(h) = hover {
            let d2 = p.distance_sq(h);
            if d2 <= 81.0 && best_hit.as_ref().is_none_or(|b| d2 < b.0) {
                best_hit = Some((d2, text()));
            }
        }
    };
    let mut taken: Vec<Rect> = Vec::new();
    let small = FontId::proportional(10.0);

    for g in sp.systems() {
        let mut runs: Vec<&Aid> = g.aids.iter().filter(|a| sp.shows(a)).collect();
        runs.sort_by_key(|a| rank(a.info().group));
        for a in runs {
            let pts = project(a.points.iter().map(|p| (p.lat, p.lon)), &to_screen);
            let bb = pts
                .iter()
                .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
            if !bb.expand(20.0).intersects(clip) {
                continue;
            }
            let (w, col) = style(a);
            let info = a.info();
            if info.group != Group::EnsembleMember {
                painter.add(Shape::line(
                    pts.iter().map(|p| *p + Vec2::splat(1.0)).collect(),
                    Stroke::new(w + 1.0, Color32::from_black_alpha(80)),
                ));
            }
            painter.add(Shape::line(pts.clone(), Stroke::new(w, col)));
            for (p, pt) in pts.iter().zip(&a.points) {
                if pt.tau > 0 && pt.tau % 24 == 0 && info.group != Group::EnsembleMember {
                    painter.circle_filled(*p, w + 1.3, col);
                    painter.circle_stroke(
                        *p,
                        w + 1.3,
                        Stroke::new(0.8, Color32::from_black_alpha(160)),
                    );
                }
                consider(*p, &|| {
                    let valid = a.cycle + Duration::hours(i64::from(pt.tau));
                    let mut s = format!(
                        "{} ({}) · {} run\n+{} h · {}",
                        if info.name.is_empty() {
                            a.tech.as_str()
                        } else {
                            info.name
                        },
                        a.tech,
                        cycle_label(a.cycle),
                        pt.tau,
                        valid_label(valid, tz)
                    );
                    match (pt.vmax_kt, pt.mslp_mb) {
                        (Some(v), Some(m)) => s.push_str(&format!("\n{v:.0} kt · {m:.0} mb")),
                        (Some(v), None) => s.push_str(&format!("\n{v:.0} kt")),
                        (None, Some(m)) => s.push_str(&format!("\n{m:.0} mb")),
                        _ => {}
                    }
                    s.push_str(&format!("\n{}", g.title()));
                    s
                });
            }
            // The model id at the end of its line, when there is room for it.
            if sp.labels && zoom >= 3.0 && info.group != Group::EnsembleMember {
                if let (Some(end), Some(prev)) = (pts.last(), pts.iter().rev().nth(1)) {
                    let dir = (*end - *prev).normalized();
                    let anchor = *end + dir * 5.0;
                    let align = match (dir.x >= 0.0, dir.y >= 0.0) {
                        (true, true) => Align2::LEFT_TOP,
                        (true, false) => Align2::LEFT_BOTTOM,
                        (false, true) => Align2::RIGHT_TOP,
                        (false, false) => Align2::RIGHT_BOTTOM,
                    };
                    let galley = painter.layout_no_wrap(a.tech.clone(), small.clone(), col);
                    let r = align.anchor_size(anchor, galley.size());
                    if clip.contains_rect(r) && !taken.iter().any(|t| t.intersects(r)) {
                        taken.push(r);
                        painter.rect_filled(r.expand(1.5), 2.0, Color32::from_black_alpha(150));
                        painter.galley(r.min, galley, col);
                    }
                }
            }
        }

        if sp.best_track && g.best.len() >= 2 {
            draw_best_track(painter, g, clip, &to_screen, &mut consider, tz);
        }
        if g.is_invest() {
            if let Some(f) = g.latest_fix() {
                let p = to_screen(f.lon, f.lat);
                if clip.expand(20.0).contains(p) {
                    invest_marker(painter, p, g, &mut taken);
                    consider(p, &|| {
                        let mut s = format!(
                            "{}\n{} · {}",
                            g.title(),
                            stage_name(&f.stage),
                            valid_label(f.time, tz)
                        );
                        if let Some(v) = f.vmax_kt {
                            s.push_str(&format!("\n{v:.0} kt"));
                        }
                        if let Some(m) = f.mslp_mb {
                            s.push_str(&format!(" · {m:.0} mb"));
                        }
                        s.push_str("\nClick for its model guidance");
                        s
                    });
                }
            }
        }
    }
    best_hit.map(|b| b.1)
}

/// A best-track stage code in words.
pub fn stage_name(s: &str) -> &str {
    match s {
        "TD" => "Tropical depression",
        "TS" => "Tropical storm",
        "HU" => "Hurricane",
        "TY" => "Typhoon",
        "ST" => "Super typhoon",
        "EX" => "Extratropical",
        "SD" => "Subtropical depression",
        "SS" => "Subtropical storm",
        "LO" => "Low",
        "DB" => "Disturbance",
        "WV" => "Tropical wave",
        "IN" => "Inland",
        _ => s,
    }
}

fn intensity_color(kt: Option<f32>) -> Color32 {
    match kt {
        Some(k) => {
            let (_, rgb) = wxdata::tropical::saffir_simpson(k);
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        }
        None => Color32::from_gray(160),
    }
}

/// The observed track: solid, each leg in the color of the intensity it reached, a dot per fix
/// (hollow where the system was not tropical: a low, a wave, extratropical).
fn draw_best_track(
    painter: &Painter,
    g: &Guidance,
    clip: Rect,
    to_screen: &impl Fn(f64, f64) -> Pos2,
    consider: &mut impl FnMut(Pos2, &dyn Fn() -> String),
    tz: Option<Tz>,
) {
    let pts = project(g.best.iter().map(|b| (b.lat, b.lon)), to_screen);
    let bb = pts
        .iter()
        .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
    if !bb.expand(20.0).intersects(clip) {
        return;
    }
    for (w, b) in pts.windows(2).zip(g.best.windows(2)) {
        painter.line_segment(
            [w[0], w[1]],
            Stroke::new(4.5, Color32::from_black_alpha(140)),
        );
        painter.line_segment(
            [w[0], w[1]],
            Stroke::new(2.6, intensity_color(b[1].vmax_kt)),
        );
    }
    for (p, b) in pts.iter().zip(&g.best) {
        let col = intensity_color(b.vmax_kt);
        let tropical = matches!(
            b.stage.as_str(),
            "TD" | "TS" | "HU" | "TY" | "ST" | "SD" | "SS"
        );
        painter.circle_filled(*p, 3.6, Color32::from_black_alpha(160));
        if tropical {
            painter.circle_filled(*p, 2.8, col);
        } else {
            painter.circle_stroke(*p, 2.6, Stroke::new(1.3, col));
        }
        consider(*p, &|| {
            let mut s = format!(
                "{} best track\n{} · {}",
                g.title(),
                valid_label(b.time, tz),
                stage_name(&b.stage)
            );
            if let Some(v) = b.vmax_kt {
                s.push_str(&format!("\n{v:.0} kt"));
            }
            if let Some(m) = b.mslp_mb {
                s.push_str(&format!(" · {m:.0} mb"));
            }
            s
        });
    }
}

/// An invest: a dashed ring (a disturbance, not a storm) and its number.
fn invest_marker(painter: &Painter, p: Pos2, g: &Guidance, taken: &mut Vec<Rect>) {
    let col = Color32::from_rgb(255, 170, 60);
    let ring: Vec<Pos2> = (0..=24)
        .map(|k| {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            p + Vec2::new(a.cos(), a.sin()) * 9.0
        })
        .collect();
    painter.add(Shape::dashed_line(
        &ring,
        Stroke::new(3.0, Color32::from_black_alpha(140)),
        3.0,
        2.5,
    ));
    painter.add(Shape::dashed_line(&ring, Stroke::new(1.8, col), 3.0, 2.5));
    painter.circle_filled(p, 2.5, col);
    let label = g.title().trim_start_matches("Invest ").to_string();
    let galley = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
    let r = Align2::LEFT_CENTER.anchor_size(p + Vec2::new(12.0, 0.0), galley.size());
    painter.rect_filled(
        r.expand(2.0),
        3.0,
        Color32::from_rgba_premultiplied(16, 19, 26, 200),
    );
    painter.galley(r.min, galley, Color32::WHITE);
    taken.push(r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::atcf::AidPoint;

    fn run(tech: &str) -> Aid {
        Aid {
            tech: tech.into(),
            cycle: Utc::now(),
            points: vec![
                AidPoint {
                    tau: 0,
                    lat: 20.0,
                    lon: -60.0,
                    vmax_kt: None,
                    mslp_mb: None,
                },
                AidPoint {
                    tau: 24,
                    lat: 22.0,
                    lon: -63.0,
                    vmax_kt: None,
                    mslp_mb: None,
                },
            ],
        }
    }

    #[test]
    fn the_picker_decides_what_draws() {
        let mut sp = Spaghetti::default();
        assert!(sp.shows(&run("AVNO")), "globals are on by default");
        assert!(
            !sp.shows(&run("AVNI")),
            "interpolated aids wait to be asked for"
        );
        assert!(!sp.shows(&run("AP05")), "members too");
        assert!(
            !sp.shows(&run("SHIP")),
            "an intensity aid never draws as a track"
        );
        sp.interpolated = true;
        assert!(sp.shows(&run("AVNI")));
        sp.hidden.insert("AVNO".into());
        assert!(!sp.shows(&run("AVNO")));
        sp.groups[group_index(Group::EnsembleMember)] = true;
        assert!(sp.shows(&run("AP05")));
        sp.groups[group_index(Group::Global)] = false;
        assert!(!sp.shows(&run("CMC")));
    }

    #[test]
    fn a_dissipated_focus_is_let_go() {
        let mut sp = Spaghetti {
            focus: Some("al982026".into()),
            ..Default::default()
        };
        sp.ingest(Ok(vec![Guidance {
            id: "al062026".into(),
            name: "FAY".into(),
            aids: vec![],
            best: vec![],
        }]));
        assert_eq!(sp.focus, None);
        assert_eq!(sp.systems().count(), 1);
        sp.ingest(Err("offline".into()));
        assert_eq!(
            sp.guidance.len(),
            1,
            "a failed refresh keeps what was there"
        );
        assert_eq!(sp.error.as_deref(), Some("offline"));
    }

    #[test]
    fn projection_keeps_a_dateline_track_continuous() {
        let pts = project([(10.0, 179.0), (11.0, -179.0)].into_iter(), &|lon, lat| {
            Pos2::new(lon as f32, lat as f32)
        });
        assert_eq!(pts[1].x, 181.0);
    }
}
