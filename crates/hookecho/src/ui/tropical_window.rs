//! The Tropical window: model guidance for every active storm and invest, and NHC's text
//! products (public advisory, forecast discussion) for the storms it is advising on.
//!
//! The Models tab is the picker for the spaghetti on the map (`crate::spaghetti`): which systems,
//! which groups of models, which individual models. It also carries what a spaghetti plot cannot
//! say on its own: how far apart the tracks are at each day (the spread is the uncertainty), and
//! what the models do with the storm's strength, on an intensity chart that runs from the best
//! track's recent history into the forecast.
//!
//! The text tabs are where the hurricane specialist says what the guidance disagrees about and
//! how confident the track really is — the part a cone cannot express.

use crate::spaghetti::{group_index, Spaghetti};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke};
use wxdata::atcf::{Aid, Group, Guidance};
use wxdata::tropical::{Advisory, TropicalStorm};
use wxdata::tz::Tz;

/// Which text product to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    /// Public advisory (TCP): watches, warnings, hazards, the numbers.
    Advisory,
    /// Forecast discussion (TCD): the forecaster's reasoning.
    Discussion,
}

impl Product {
    pub fn label(self) -> &'static str {
        match self {
            Product::Advisory => "Advisory",
            Product::Discussion => "Discussion",
        }
    }

    /// This product's page for `storm`, if the feed published one.
    pub fn url(self, storm: &TropicalStorm) -> Option<&str> {
        match self {
            Product::Advisory => storm.advisory_url.as_deref(),
            Product::Discussion => storm.discussion_url.as_deref(),
        }
    }
}

/// The window's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Models,
    Text,
}

/// What the window is currently pointed at.
pub struct TropicalWindow {
    pub open: bool,
    pub tab: Tab,
    /// Storm id the text belongs to; `None` until one is chosen.
    pub storm_id: Option<String>,
    pub product: Product,
    pub text: Option<Advisory>,
    pub busy: bool,
    pub error: Option<String>,
}

impl Default for TropicalWindow {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Models,
            storm_id: None,
            product: Product::Discussion,
            text: None,
            busy: false,
            error: None,
        }
    }
}

/// What the window asks of the app this frame.
#[derive(Default)]
pub struct Out {
    /// Fetch a text product.
    pub fetch: Option<(String, Product)>,
    /// Center the map here.
    pub center: Option<(f64, f64)>,
    /// Refetch the model guidance now.
    pub refresh_models: bool,
}

/// Show the window.
pub fn show(
    w: &mut TropicalWindow,
    ctx: &egui::Context,
    storms: &[TropicalStorm],
    sp: &mut Spaghetti,
    tz: Option<Tz>,
    drawer: &mut crate::ui::drawer::Drawer,
) -> Out {
    let mut out = Out::default();
    if !w.open {
        return out;
    }
    let mut open = w.open;
    let Some(window) = drawer.page_sized(
        ctx,
        "Tropical",
        &mut open,
        false,
        620.0,
        egui::Window::new("Tropical"),
    ) else {
        w.open = open;
        return out;
    };
    window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut w.tab, Tab::Models, "Models");
            ui.selectable_value(&mut w.tab, Tab::Text, "Advisories & discussions");
        });
        ui.separator();
        match w.tab {
            Tab::Models => models_tab(ui, w, sp, tz, &mut out),
            Tab::Text => text_tab(ui, w, storms, &mut out),
        }
    });
    w.open = open;
    out
}

fn text_tab(ui: &mut egui::Ui, w: &mut TropicalWindow, storms: &[TropicalStorm], out: &mut Out) {
    if storms.is_empty() {
        ui.weak("No tropical cyclones are being advised on.");
        ui.small("The NHC publishes these only while a storm is being advised on. Invests have model guidance (the Models tab) but no advisories.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for s in storms {
            let selected = w.storm_id.as_deref() == Some(s.id.as_str());
            if ui
                .selectable_label(selected, format!("{} {}", s.classification, s.name))
                .clicked()
                && !selected
            {
                w.storm_id = Some(s.id.clone());
                out.fetch = Some((s.id.clone(), w.product));
            }
        }
    });
    // A storm picked on the Models tab (or an invest) has no text loaded yet.
    if let Some(id) = &w.storm_id {
        let advised = storms.iter().any(|s| &s.id == id);
        let loaded = w.text.is_some() || w.busy || w.error.is_some();
        if advised && !loaded {
            out.fetch = Some((id.clone(), w.product));
        }
    }
    ui.horizontal(|ui| {
        for p in [Product::Discussion, Product::Advisory] {
            if ui.selectable_label(w.product == p, p.label()).clicked() && w.product != p {
                w.product = p;
                if let Some(id) = w.storm_id.clone() {
                    out.fetch = Some((id, p));
                }
            }
        }
        if w.busy {
            crate::ui::loading(ui, "Fetching…");
        } else if ui.button("⟳ Refresh").clicked() {
            if let Some(id) = w.storm_id.clone() {
                out.fetch = Some((id, w.product));
            }
        }
        if let Some(a) = &w.text {
            crate::ui::reader_button(ui, &a.title, w.product.label(), &a.text);
        }
    });
    if let Some(e) = &w.error {
        ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match &w.text {
            // The products are column-formatted plain text; monospace or they lose it.
            Some(a) => {
                ui.add(
                    egui::Label::new(egui::RichText::new(&a.text).monospace().size(12.0)).wrap(),
                );
            }
            None if w.busy => {
                ui.weak("Fetching…");
            }
            None => {
                ui.weak("Pick a storm to read its latest product.");
            }
        });
}

fn swatch(ui: &mut egui::Ui, rgb: [u8; 3]) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 10.0), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_center_size(r.center(), egui::vec2(14.0, 3.5)),
        1.0,
        Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
    );
}

fn models_tab(
    ui: &mut egui::Ui,
    w: &mut TropicalWindow,
    sp: &mut Spaghetti,
    tz: Option<Tz>,
    out: &mut Out,
) {
    ui.horizontal_wrapped(|ui| {
        crate::ui::style::toggle(ui, &mut sp.enabled, "Model tracks on the map");
        if sp.loading {
            crate::ui::loading(ui, "Fetching guidance…");
        } else if sp.enabled && ui.button("⟳ Refresh").clicked() {
            out.refresh_models = true;
        }
    });
    if !sp.enabled {
        ui.weak("Turn the model tracks on to load every active storm's and invest's guidance: the global and hurricane models, consensus aids, ensemble means and members, and the observed track.");
        return;
    }
    if let Some(e) = &sp.error {
        ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {e}"));
    }
    if sp.guidance.is_empty() {
        if !sp.loading {
            ui.weak("No active storms or invests: NHC is not running models on anything.");
        }
        return;
    }
    // Which system: one, or every one on the map.
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(sp.focus.is_none(), "All systems")
            .clicked()
        {
            sp.focus = None;
        }
        let ids: Vec<(String, String)> = sp
            .guidance
            .iter()
            .map(|g| (g.id.clone(), g.title()))
            .collect();
        for (id, title) in ids {
            let on = sp.focus.as_deref() == Some(id.as_str());
            if ui.selectable_label(on, title).clicked() {
                sp.focus = Some(id.clone());
                w.storm_id = Some(id);
                w.text = None;
                w.error = None;
            }
        }
    });
    // The detail below is for one system: the focused one, else the one picked on the map, else
    // the first.
    let id = sp
        .focus
        .clone()
        .or_else(|| w.storm_id.clone().filter(|i| sp.find(i).is_some()))
        .or_else(|| sp.guidance.first().map(|g| g.id.clone()));
    let Some(g) = id.and_then(|i| sp.find(&i)).cloned() else {
        return;
    };
    ui.separator();
    system_header(ui, &g, tz, out);
    ui.horizontal_wrapped(|ui| {
        crate::ui::style::toggle(ui, &mut sp.best_track, "Observed track");
        crate::ui::style::toggle(ui, &mut sp.interpolated, "Interpolated (early) aids")
            .on_hover_text("The previous cycle of each late model, shifted to start at the storm's current position: what forecasters use at advisory time");
        crate::ui::style::toggle(ui, &mut sp.labels, "Model labels");
    });
    spread_line(ui, &g, sp);
    ui.add_space(4.0);
    ui.label(RichText::new("Intensity guidance").strong());
    intensity_chart(ui, &g, sp, tz);
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("tropical-models")
        .max_height(360.0)
        .auto_shrink([false, true])
        .show(ui, |ui| model_picker(ui, &g, sp));
}

fn system_header(ui: &mut egui::Ui, g: &Guidance, tz: Option<Tz>, out: &mut Out) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(g.title()).size(16.0).strong());
        if let Some(f) = g.latest_fix() {
            let (cat, rgb) = f
                .vmax_kt
                .map(wxdata::tropical::saffir_simpson)
                .unwrap_or(("", [150, 150, 150]));
            let mut s = String::new();
            if let Some(v) = f.vmax_kt {
                s.push_str(&format!("{v:.0} kt"));
            }
            if let Some(m) = f.mslp_mb {
                s.push_str(&format!(" · {m:.0} mb"));
            }
            let lat = format!(
                "{:.1}°{}",
                f.lat.abs(),
                if f.lat >= 0.0 { "N" } else { "S" }
            );
            let lon = format!(
                "{:.1}°{}",
                f.lon.abs(),
                if f.lon >= 0.0 { "E" } else { "W" }
            );
            ui.label(
                // "Hurricane Cat 3"; a storm or depression's category is its stage already.
                RichText::new(if cat.starts_with("Cat") {
                    format!("{} {cat}", crate::spaghetti::stage_name(&f.stage))
                } else {
                    crate::spaghetti::stage_name(&f.stage).to_string()
                })
                .color(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
                .strong(),
            );
            ui.label(format!("{s}   {lat} {lon}"));
            ui.label(
                RichText::new(format!(
                    "as of {}",
                    crate::timefmt::fmt_date_clock(f.time, tz)
                ))
                .weak()
                .size(11.0),
            );
            if ui.small_button("⌖ Center map").clicked() {
                out.center = Some((f.lat, f.lon));
            }
        }
    });
    if let Some(c) = g.newest_cycle() {
        ui.label(
            RichText::new(format!(
                "{} model runs, newest cycle {}",
                g.aids.len(),
                c.format("%HZ %b %-d")
            ))
            .size(11.0)
            .weak(),
        );
    }
}

/// How far apart the drawn deterministic tracks are at each day, and the ensemble members'
/// spread where members are loaded: the width of the spaghetti is the uncertainty in the track.
fn spread_line(ui: &mut egui::Ui, g: &Guidance, sp: &Spaghetti) {
    let det: Vec<&Aid> = g
        .aids
        .iter()
        .filter(|a| sp.shows(a) && a.info().group != Group::EnsembleMember)
        .collect();
    let members: Vec<&Aid> = g
        .aids
        .iter()
        .filter(|a| a.info().group == Group::EnsembleMember)
        .collect();
    let row = |aids: &[&Aid]| -> Vec<String> {
        [48.0, 72.0, 120.0]
            .iter()
            .filter_map(|t| {
                wxdata::atcf::spread_nm(aids, *t)
                    .map(|(nm, n)| format!("{:.0} h: {nm:.0} nm ({n})", t))
            })
            .collect()
    };
    let d = row(&det);
    if !d.is_empty() {
        ui.label(RichText::new(format!("Track spread, drawn models — {}", d.join(" · "))).size(11.5))
            .on_hover_text("Mean distance of each model's position from their average at that hour, with how many models reach it. A wide spread is an uncertain track.");
    }
    let m = row(&members);
    if !m.is_empty() {
        ui.label(RichText::new(format!("Ensemble member spread — {}", m.join(" · "))).size(11.5));
    }
}

/// Hours from `origin` to `t`.
fn hours(origin: DateTime<Utc>, t: DateTime<Utc>) -> f64 {
    (t - origin).num_minutes() as f64 / 60.0
}

/// Wind speed against time: the best track's last two days, then each drawn model's forecast
/// (and the intensity aids when their group is on), over the Saffir–Simpson thresholds.
fn intensity_chart(ui: &mut egui::Ui, g: &Guidance, sp: &Spaghetti, tz: Option<Tz>) {
    let Some(origin) = g.newest_cycle().or(g.latest_fix().map(|f| f.time)) else {
        return;
    };
    let runs: Vec<&Aid> = g
        .aids
        .iter()
        .filter(|a| {
            let i = a.info();
            let on = if i.group == Group::Other {
                sp.group_on(Group::Other) && !sp.hidden.contains(&a.tech)
            } else {
                sp.shows(a)
            };
            on && i.group != Group::EnsembleMember && a.points.iter().any(|p| p.vmax_kt.is_some())
        })
        .collect();
    let best: Vec<(f64, f32)> = g
        .best
        .iter()
        .filter_map(|b| Some((hours(origin, b.time), b.vmax_kt?)))
        .filter(|(h, _)| *h >= -48.0)
        .collect();
    if runs.is_empty() && best.is_empty() {
        ui.weak("No intensity forecasts among the drawn models.");
        return;
    }
    let x_max = runs
        .iter()
        .filter_map(|a| {
            a.points
                .last()
                .map(|p| hours(origin, a.cycle) + f64::from(p.tau))
        })
        .fold(24.0, f64::max)
        .min(168.0);
    let x_min = best.first().map_or(0.0, |b| b.0.min(0.0));
    let v_max = runs
        .iter()
        .flat_map(|a| a.points.iter().filter_map(|p| p.vmax_kt))
        .chain(best.iter().map(|b| b.1))
        .fold(70.0f32, f32::max);
    let y_max = f64::from(v_max) * 1.12;
    let width = ui.available_width().max(300.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 200.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let vis = ui.visuals();
    let (text, weak) = (vis.text_color(), vis.weak_text_color());
    painter.rect_filled(rect, 4.0, vis.extreme_bg_color);
    let plot = Rect::from_min_max(
        rect.min + egui::vec2(34.0, 8.0),
        rect.max - egui::vec2(38.0, 20.0),
    );
    let xs = |h: f64| plot.left() + ((h - x_min) / (x_max - x_min)) as f32 * plot.width();
    let ys = |kt: f64| plot.bottom() - (kt / y_max) as f32 * plot.height();
    let small = FontId::proportional(10.0);
    // Saffir–Simpson thresholds.
    for (kt, label) in [
        (34.0, "TS"),
        (64.0, "Cat 1"),
        (83.0, "Cat 2"),
        (96.0, "Cat 3"),
        (113.0, "Cat 4"),
        (137.0, "Cat 5"),
    ] {
        if kt > y_max {
            break;
        }
        let (_, rgb) = wxdata::tropical::saffir_simpson(kt as f32);
        let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let y = ys(kt);
        painter.extend(egui::Shape::dashed_line(
            &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.8, c.gamma_multiply(0.7)),
            4.0,
            4.0,
        ));
        painter.text(
            Pos2::new(plot.right() + 3.0, y),
            Align2::LEFT_CENTER,
            label,
            small.clone(),
            c,
        );
    }
    // Wind axis.
    let mut kt = 0.0;
    while kt <= y_max {
        painter.text(
            Pos2::new(plot.left() - 4.0, ys(kt)),
            Align2::RIGHT_CENTER,
            format!("{kt:.0}"),
            small.clone(),
            weak,
        );
        kt += 20.0;
    }
    painter.text(
        rect.left_top() + egui::vec2(3.0, 2.0),
        Align2::LEFT_TOP,
        "kt",
        small.clone(),
        weak,
    );
    // Time axis: a tick a day at the cycle hour, labeled by day.
    let mut h = (x_min / 24.0).ceil() * 24.0;
    while h <= x_max {
        let x = xs(h);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(
                0.5,
                vis.widgets
                    .noninteractive
                    .bg_stroke
                    .color
                    .gamma_multiply(0.6),
            ),
        );
        let t = origin + Duration::hours(h as i64);
        let day = match tz {
            Some(z) => t.with_timezone(&z).format("%a").to_string(),
            None => t.format("%a").to_string(),
        };
        let label = if h == 0.0 {
            format!("{day} (+0)")
        } else {
            format!("{day} {h:+.0}")
        };
        painter.text(
            Pos2::new(x, plot.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            weak,
        );
        h += 24.0;
    }
    // Now.
    let xn = xs(hours(origin, Utc::now()));
    if (plot.left()..=plot.right()).contains(&xn) {
        painter.line_segment(
            [Pos2::new(xn, plot.top()), Pos2::new(xn, plot.bottom())],
            Stroke::new(1.0, weak),
        );
    }
    // The models, then the best track over them.
    let mut near: Option<(f32, String)> = None;
    let hover = response.hover_pos().filter(|p| plot.contains(*p));
    let mut consider = |p: Pos2, s: &dyn Fn() -> String| {
        if let Some(hp) = hover {
            let d = p.distance_sq(hp);
            if d < 100.0 && near.as_ref().is_none_or(|n| d < n.0) {
                near = Some((d, s()));
            }
        }
    };
    for a in &runs {
        let i = a.info();
        let col = Color32::from_rgb(i.rgb[0], i.rgb[1], i.rgb[2]);
        let off = hours(origin, a.cycle);
        let pts: Vec<(Pos2, &wxdata::atcf::AidPoint)> = a
            .points
            .iter()
            .filter_map(|p| {
                let v = p.vmax_kt?;
                let h = off + f64::from(p.tau);
                (h <= x_max).then(|| (Pos2::new(xs(h), ys(f64::from(v))), p))
            })
            .collect();
        let width = if matches!(i.group, Group::Official | Group::Consensus) {
            2.2
        } else {
            1.4
        };
        painter.add(egui::Shape::line(
            pts.iter().map(|p| p.0).collect(),
            Stroke::new(width, col),
        ));
        for (p, pt) in &pts {
            consider(*p, &|| {
                format!(
                    "{} ({}) {} run\n+{} h: {:.0} kt",
                    if i.name.is_empty() {
                        a.tech.as_str()
                    } else {
                        i.name
                    },
                    a.tech,
                    a.cycle.format("%HZ"),
                    pt.tau,
                    pt.vmax_kt.unwrap_or(0.0)
                )
            });
        }
    }
    if best.len() >= 2 {
        let pts: Vec<Pos2> = best
            .iter()
            .map(|(h, v)| Pos2::new(xs(*h), ys(f64::from(*v))))
            .collect();
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0, text)));
        for (p, (h, v)) in pts.iter().zip(&best) {
            painter.circle_filled(*p, 2.5, text);
            consider(*p, &|| format!("Best track {h:+.0} h: {v:.0} kt"));
        }
    }
    if let Some((_, s)) = near {
        response.on_hover_text_at_pointer(s);
    }
}

/// Group sections, each with its models: a swatch, a checkbox, the name and id, the run, and
/// what the run does with the storm (its peak wind and when).
fn model_picker(ui: &mut egui::Ui, g: &Guidance, sp: &mut Spaghetti) {
    for group in Group::ALL {
        // Interpolated aids are listed only while they are shown (except the intensity aids,
        // which are never drawn as tracks anyway).
        let mut runs: Vec<&Aid> = g
            .aids
            .iter()
            .filter(|a| {
                let i = a.info();
                i.group == group && (sp.interpolated || !i.interpolated || group == Group::Other)
            })
            .collect();
        if runs.is_empty() {
            continue;
        }
        runs.sort_by(|a, b| {
            a.info()
                .interpolated
                .cmp(&b.info().interpolated)
                .then(a.tech.cmp(&b.tech))
        });
        let gi = group_index(group);
        let title = if group == Group::Other {
            format!("{} ({}) — intensity chart only", group.label(), runs.len())
        } else {
            format!("{} ({})", group.label(), runs.len())
        };
        let id = ui.make_persistent_id(("tropical-group", gi));
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            group != Group::EnsembleMember && group != Group::Other,
        )
        .show_header(ui, |ui| {
            ui.checkbox(&mut sp.groups[gi], RichText::new(title).strong());
        })
        .body(|ui| {
            if group == Group::EnsembleMember {
                ui.horizontal(|ui| {
                    if ui.small_button("All").clicked() {
                        for a in &runs {
                            sp.hidden.remove(&a.tech);
                        }
                    }
                    if ui.small_button("None").clicked() {
                        for a in &runs {
                            sp.hidden.insert(a.tech.clone());
                        }
                    }
                });
            }
            for a in runs {
                let i = a.info();
                ui.horizontal(|ui| {
                    swatch(ui, i.rgb);
                    let mut on = !sp.hidden.contains(&a.tech);
                    let name = if i.name.is_empty() {
                        a.tech.clone()
                    } else {
                        format!("{} ({})", i.name, a.tech)
                    };
                    let enabled = sp.groups[gi];
                    if ui
                        .add_enabled(enabled, egui::Checkbox::new(&mut on, name))
                        .changed()
                    {
                        if on {
                            sp.hidden.remove(&a.tech);
                        } else {
                            sp.hidden.insert(a.tech.clone());
                        }
                    }
                    let peak = a
                        .points
                        .iter()
                        .filter_map(|p| p.vmax_kt.map(|v| (v, p.tau)))
                        .max_by(|x, y| x.0.total_cmp(&y.0));
                    let mut note = format!(
                        "{} run · to +{} h",
                        a.cycle.format("%HZ"),
                        a.points.last().map_or(0, |p| p.tau)
                    );
                    if let Some((v, t)) = peak {
                        note.push_str(&format!(" · peak {v:.0} kt at +{t} h"));
                    }
                    ui.label(RichText::new(note).size(10.5).weak());
                });
            }
        });
    }
}
