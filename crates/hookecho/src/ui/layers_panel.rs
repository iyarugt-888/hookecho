//! The Layers panel: a searchable, categorized list of every layer/product/tool in the app,
//! browsed through category tiles or the active list. Desktop uses floating cards; Android
//! hosts the same body in a bottom sheet. Both read the one action registry
//! (`HookEchoApp::palette_entries`), so they can never drift apart.

use crate::app::{HealthState, PaletteAction, PaletteEntry, SourceHealth};
use crate::ui::a11y::Named as _;
use egui::{vec2, Color32, RichText, Stroke};

/// Category order in the panel (anything else falls to the bottom, in registry order).
pub(crate) const CATEGORIES: [&str; 8] = [
    "Radar",
    "Sites",
    "National",
    "Severe",
    "Obs",
    "Models",
    "Reference",
    "Tools",
];

/// Case-insensitive subsequence match with a compactness score: lower is a tighter match.
/// `None` = no match. Empty needle matches everything at score 0.
pub(crate) fn fuzzy(needle: &str, hay: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let mut score = 0usize;
    let mut at = 0usize;
    for nc in needle.to_lowercase().chars() {
        if nc == ' ' {
            continue;
        }
        let found = hay[at..].iter().position(|h| *h == nc)?;
        score += found; // characters skipped between matches — tighter matches score lower
        at += found + 1;
    }
    Some(score)
}

/// Filter + sort entry indices for `query` (best match first, registry order within a tie).
pub(crate) fn matches(entries: &[PaletteEntry], query: &str) -> Vec<usize> {
    let trimmed = query.trim();
    let lower = trimmed.to_ascii_lowercase();
    let (scope, needle) = if lower.starts_with("switch station ") {
        (Some("Sites"), &trimmed[15..])
    } else if lower.starts_with("station ") {
        (Some("Sites"), &trimmed[8..])
    } else if lower.starts_with("site ") {
        (Some("Sites"), &trimmed[5..])
    } else if lower.starts_with("tool ") {
        (Some("Tools"), &trimmed[5..])
    } else {
        (None, trimmed)
    };
    let mut hits: Vec<(usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if scope.is_some_and(|category| e.category != category) {
                return None;
            }
            let metadata = match e.action {
                PaletteAction::ToggleField(layer) => layer.descriptor(),
                _ => None,
            };
            let score = fuzzy(needle, &e.label).or_else(|| {
                metadata
                    .and_then(|field| fuzzy(needle, &field.search_text()))
                    .map(|score| score.saturating_add(1000))
            });
            score.map(|score| (score, i))
        })
        .collect();
    hits.sort_by_key(|(s, i)| (*s, *i));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// Typed timeline commands share the same result list as the action registry. Times without a
/// date use the day currently selected on the radar timeline; every displayed time is UTC.
fn command_entry(query: &str, selected_day: chrono::NaiveDate) -> Option<PaletteEntry> {
    let (verb, value) = query.trim().split_once(' ')?;
    if !["time", "at", "goto"]
        .iter()
        .any(|word| verb.eq_ignore_ascii_case(word))
    {
        return None;
    }
    let value = value.trim();
    let (label, action, desc) = if value.eq_ignore_ascii_case("live")
        || value.eq_ignore_ascii_case("now")
    {
        (
            "Return timeline to live".to_string(),
            PaletteAction::GoLive,
            "Resume the newest radar scan",
        )
    } else {
        let target = parse_utc_time(value, selected_day)?;
        (
            format!("Seek timeline to {} UTC", target.format("%Y-%m-%d %H:%M")),
            PaletteAction::SeekTime(target.timestamp()),
            "Seek the active radar pane to the nearest scan at this UTC time",
        )
    };
    Some(PaletteEntry {
        label,
        category: "Tools",
        action,
        on: None,
        desc,
        common: false,
        key: None,
        health: None,
    })
}

fn parse_utc_time(
    value: &str,
    selected_day: chrono::NaiveDate,
) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(time) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(time.with_timezone(&chrono::Utc));
    }
    let value = value.trim_end_matches(['Z', 'z']);
    for pattern in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(time) = chrono::NaiveDateTime::parse_from_str(value, pattern) {
            return Some(time.and_utc());
        }
    }
    if let Ok(day) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return day.and_hms_opt(0, 0, 0).map(|time| time.and_utc());
    }
    for pattern in ["%H:%M:%S", "%H:%M"] {
        if let Ok(time) = chrono::NaiveTime::parse_from_str(value, pattern) {
            return Some(selected_day.and_time(time).and_utc());
        }
    }
    None
}

/// Row height: one line, tall enough to scan without turning the panel into a wall.
const ROW_H: f32 = 32.0;

fn category_name(category: &str) -> &'static str {
    match category {
        "Radar" => "Radar products",
        "Sites" => "Radar sites",
        "National" => "National weather",
        "Severe" => "Severe weather",
        "Obs" => "Observations",
        "Models" => "Forecast models",
        "Reference" => "Map reference",
        _ => "Tools",
    }
}

fn category_glyph(category: &str) -> &'static str {
    use egui_phosphor::regular as ph;
    match category {
        "Radar" => ph::BROADCAST,
        "Sites" => ph::MAP_PIN,
        "National" => ph::GLOBE,
        "Severe" => ph::WARNING,
        "Obs" => ph::THERMOMETER,
        "Models" => ph::CHART_LINE,
        "Reference" => ph::MAP_TRIFOLD,
        _ => ph::WRENCH,
    }
}

/// The row's icon, picked from the label and falling back to the category.
///
/// Derived rather than stored: a per-entry `icon` field would be ~100 registry edits to keep in
/// sync by hand, and the labels already say what the thing is.
pub(crate) fn glyph(e: &PaletteEntry) -> &'static str {
    use egui_phosphor::regular as ph;
    let l = e.label.to_lowercase();
    let has = |w: &str| l.contains(w);
    match () {
        _ if has("velocity")
            || has("azshear")
            || has("rotation")
            || has("srv")
            || has("srh")
            || has("spin") =>
        {
            ph::ARROWS_CLOCKWISE
        }
        _ if has("hail") || has("mesh") => ph::CIRCLE,
        _ if has("tornado") || has("tds") => ph::TORNADO,
        _ if has("lightning") || has("glm") => ph::LIGHTNING,
        _ if has("snow") || has("winter") || has("ice") => ph::SNOWFLAKE,
        _ if has("rain")
            || has("qpe")
            || has("precip")
            || has("flood")
            || has("vil")
            || has("moisture") =>
        {
            ph::DROP
        }
        _ if has("wind") => ph::WIND,
        _ if has("temp") || has("dewpoint") => ph::THERMOMETER,
        _ if has("satellite") || has("cloud") || has("smoke") => ph::CLOUD,
        _ if has("surge") || has("buoy") || has("wave") || has("river") => ph::WAVES,
        _ if has("pirep") || has("sigmet") || has("airmet") || has("recon") => ph::AIRPLANE_TILT,
        _ if has("warning") || has("alert") || has("outlook") || has("watch") => ph::WARNING,
        _ if has("sounding") || has("vad") || has("cape") || has("chart") => ph::CHART_LINE,
        _ if has("cross-section") || has("3d") || has("cappi") || has("volume") => ph::CUBE,
        _ if has("measure") || has("range") || has("distance") => ph::RULER,
        _ if has("marker") || has("place") || has("gauge") || has("spotter") => ph::MAP_PIN,
        _ if has("basemap") || has("map") || has("terrain") => ph::MAP_TRIFOLD,
        _ if has("site") || has("radar site") || has("mosaic") => ph::BROADCAST,
        _ if has("camera") || has("webcam") => ph::CAMERA,
        _ if has("setting") || has("preference") => ph::GEAR,
        _ => match e.category {
            "Radar" => ph::RADIO_BUTTON,
            "National" => ph::GLOBE,
            "Severe" => ph::WARNING,
            "Obs" => ph::THERMOMETER,
            "Models" => ph::CHART_LINE,
            "Reference" => ph::MAP_TRIFOLD,
            _ => ph::CROSSHAIR,
        },
    }
}

/// Move `drag` to sit where `before` is inside `seq`, and record the result in `pref` — the
/// persisted, cross-category label order. Only the moved category's labels are rewritten, so
/// reordering Radar leaves an earlier drag in Obs alone.
pub(crate) fn reorder(pref: &mut Vec<String>, seq: &[String], drag: &str, before: &str) {
    if drag == before {
        return;
    }
    let mut next: Vec<String> = seq.iter().filter(|s| *s != drag).cloned().collect();
    let at = next.iter().position(|s| s == before).unwrap_or(next.len());
    next.insert(at, drag.to_string());
    pref.retain(|s| !seq.iter().any(|q| q == s));
    pref.extend(next);
}

/// How many slugs [`note_recent`] keeps. Enough to matter (a session usually cycles through a
/// handful of products) without turning "Recent" into a second copy of the whole catalog.
pub(crate) const RECENT_LAYERS_CAP: usize = 6;

/// Record that `slug` was just turned on, most-recent-first. Re-toggling something already in the
/// list moves it to the front rather than duplicating it — the point is "what have I been
/// looking at", not a tally of how many times.
pub(crate) fn note_recent(recent: &mut Vec<String>, slug: &str) {
    recent.retain(|s| s != slug);
    recent.insert(0, slug.to_string());
    recent.truncate(RECENT_LAYERS_CAP);
}

/// The catalog rows named by `recent`, in recency order, dropping any slug that no longer
/// resolves to an entry — a renamed or removed action must not leave a dead row the user can
/// click into nothing, the same failure mode [`reorder`]'s own doc comment guards `layer_order`
/// against.
pub(crate) fn recent_entries<'a>(entries: &'a [PaletteEntry], recent: &[String]) -> Vec<&'a PaletteEntry> {
    recent
        .iter()
        .filter_map(|slug| {
            entries.iter().find(|e| match e.action {
                PaletteAction::ToggleField(l) => l.slug() == slug,
                _ => false,
            })
        })
        .collect()
}

/// Flip whether `slug` is starred — the star affordance on every row (`row`'s trailing column)
/// calls this directly rather than routing through a `PaletteAction`, since favoriting is pure
/// bookkeeping with no effect on the layer itself, same reasoning as [`reorder`] mutating
/// `layer_order` in place instead of becoming an action.
pub(crate) fn toggle_favorite(favorites: &mut Vec<String>, slug: &str) {
    if let Some(pos) = favorites.iter().position(|s| s == slug) {
        favorites.remove(pos);
    } else {
        favorites.push(slug.to_string());
    }
}

/// The catalog rows named by `favorites`, oldest-starred-first, dropping any slug that no longer
/// resolves to an entry — same reasoning as [`recent_entries`].
pub(crate) fn favorite_entries<'a>(
    entries: &'a [PaletteEntry],
    favorites: &[String],
) -> Vec<&'a PaletteEntry> {
    favorites
        .iter()
        .filter_map(|slug| {
            entries.iter().find(|e| match e.action {
                PaletteAction::ToggleField(l) => l.slug() == slug,
                _ => false,
            })
        })
        .collect()
}

/// One full-width row: the name, a state dot on the right, the description on hover. It used to
/// be a two-line 52 px card, which turned a category into a wall and pushed everything below the
/// fold; the description is a hint, not something you read twenty times in a row.
/// `draggable` puts the icon on a drag handle; the returned response covers the whole row and is
/// what the caller tests for a drop.
/// What a row click did: toggled the layer, or asked what the label's abbreviation means.
struct Hit {
    clicked: bool,
    /// Index into [`crate::ui::glossary::ENTRIES`], when the ⓘ was the thing clicked.
    explain: Option<usize>,
    /// The slug to flip favorite-status on, when the star was the thing clicked instead of the
    /// row itself.
    toggle_favorite: Option<String>,
    resp: egui::Response,
}

fn compact_age(age: std::time::Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0..=4 => "now".into(),
        5..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86_399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

pub(crate) fn health_look(state: HealthState) -> (&'static str, Color32) {
    match state {
        HealthState::Fresh => ("Fresh", Color32::from_rgb(70, 200, 120)),
        HealthState::Fetching => ("Fetching", Color32::from_rgb(80, 160, 240)),
        HealthState::Stale => ("Stale", Color32::from_rgb(235, 180, 70)),
        HealthState::Failed => ("Failed", Color32::from_rgb(230, 90, 90)),
        HealthState::Waiting => ("Waiting", Color32::from_gray(110)),
    }
}

fn age_line(age: Option<std::time::Duration>) -> String {
    age.map_or_else(|| "never".into(), |d| format!("{} ago", compact_age(d)))
}

fn health_popup(ui: &mut egui::Ui, health: &SourceHealth) {
    let state = health.state();
    let (label, color) = health_look(state);
    ui.set_min_width(250.0);
    ui.strong(&health.source);
    ui.colored_label(
        color,
        if state == HealthState::Failed && health.last_success.is_some() {
            "Failed — showing previous data (degraded)"
        } else {
            label
        },
    );
    egui::Grid::new(("source_health", &health.source))
        .num_columns(2)
        .show(ui, |ui| {
            ui.weak("Last success");
            ui.label(age_line(health.last_success));
            ui.end_row();
            ui.weak("Last attempt");
            ui.label(age_line(health.last_attempt));
            ui.end_row();
            ui.weak("Next retry");
            ui.label(if health.fetching {
                "in progress".into()
            } else {
                health
                    .next_retry()
                    .map_or_else(|| "waiting".into(), |d| format!("in {}", compact_age(d)))
            });
            ui.end_row();
            for (label, value) in &health.details {
                ui.weak(*label);
                ui.label(value);
                ui.end_row();
            }
        });
    if let Some(error) = &health.error {
        ui.separator();
        ui.weak("Last error");
        ui.colored_label(Color32::from_rgb(230, 120, 120), error);
    }
}

fn search_context(action: PaletteAction) -> &'static str {
    match action {
        PaletteAction::SetMoment(..) => "Radar product",
        PaletteAction::SetSite(..) => "Radar station",
        PaletteAction::SeekTime(..) | PaletteAction::GoLive => "Timeline",
        PaletteAction::Tool(..) => "Map tool",
        PaletteAction::ToggleField(..) => "Weather layer",
        PaletteAction::ToggleOverlay(..) => "Map overlay",
        PaletteAction::OpenWindow(..) => "Window",
        _ => "Command",
    }
}

fn row(
    ui: &mut egui::Ui,
    e: &PaletteEntry,
    accent: Color32,
    draggable: bool,
    search: bool,
    favorites: &[String],
) -> Hit {
    let on = e.on.unwrap_or(false);
    let (fg, bg) = if on {
        (
            accent,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 24),
        )
    } else {
        (ui.visuals().text_color(), ui.visuals().faint_bg_color)
    };
    let icon = RichText::new(glyph(e)).size(14.0).color(if on {
        accent
    } else {
        ui.visuals().weak_text_color()
    });
    // Only a `ToggleField` layer has a slug `favorite_layers` can name — a tool or a bare
    // action (e.g. "Fly to…") has nothing stable to remember it by, and nothing to look it back
    // up against on the landing screen's FAVORITES section either.
    let favorite_slug = match e.action {
        PaletteAction::ToggleField(layer) => Some(layer.slug()),
        _ => None,
    };
    let mut clicked = false;
    let mut toggle_favorite = None;
    let outer = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // The icon doubles as the grip: a separate handle column costs width the label needs,
            // and dragging from the label itself would fight the click that toggles the layer.
            if draggable {
                ui.dnd_drag_source(
                    egui::Id::new(("layer_drag", &e.label)),
                    e.label.clone(),
                    |ui| ui.label(icon),
                )
                .response
                .on_hover_cursor(egui::CursorIcon::Grab)
                .on_hover_text("Drag to reorder");
            } else {
                ui.label(icon);
            }
            // A real trailing column for the star, not a painter overlay like the explain/health
            // marks below: those are informational and can afford to sit over long label text,
            // but a clickable control needs its own reserved space so it never fights the row's
            // own click for the same pixels.
            const STAR_W: f32 = 22.0;
            let star_w = if favorite_slug.is_some() { STAR_W } else { 0.0 };
            let w = (ui.available_width() - star_w).max(0.0);
            // A justified child layout, not a plain `add`: inside a horizontal row egui centers a
            // button's text, and a column of centered labels is unreadable.
            let mut resp = ui
                .allocate_ui_with_layout(
                    vec2(w, ROW_H),
                    egui::Layout::top_down_justified(egui::Align::LEFT),
                    |ui| {
                        let label = if search {
                            format!("{}  ·  {}", search_context(e.action), e.label)
                        } else {
                            e.label.clone()
                        };
                        ui.add(
                            egui::Button::new(RichText::new(label).size(13.0).color(fg))
                                .min_size(vec2(w, ROW_H))
                                .fill(bg)
                                .corner_radius(7.0)
                                .stroke(if on {
                                    Stroke::new(1.0, accent.gamma_multiply(0.7))
                                } else {
                                    Stroke::NONE
                                }),
                        )
                    },
                )
                .inner;
            if let Some(key) = &e.key {
                resp = resp.on_hover_text(format!("{}\nShortcut: {key}", e.desc));
            } else if !e.desc.is_empty() {
                resp = resp.on_hover_text(e.desc);
            }
            clicked = resp.clicked();
            if let Some(slug) = favorite_slug {
                let is_fav = favorites.iter().any(|s| s == slug);
                let star = ui
                    .add(
                        egui::Button::new(
                            RichText::new(egui_phosphor::regular::STAR)
                                .size(14.0)
                                .color(if is_fav {
                                    accent
                                } else {
                                    ui.visuals().weak_text_color()
                                }),
                        )
                        .min_size(vec2(STAR_W, ROW_H))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                    )
                    .on_hover_text(if is_fav {
                        "Remove from Favorites"
                    } else {
                        "Add to Favorites"
                    });
                if star.clicked() {
                    toggle_favorite = Some(slug.to_string());
                }
            }
            resp
        })
        .inner;
    // The button's rect, not the whole strip: it's what the chips are drawn against and what a
    // drop is tested on, and it covers everything but the grip.
    let resp = outer;
    // ⓘ for a row whose label names a term the glossary defines, drawn over the button the same
    // way the state dot is. Clicking it explains instead of toggling: the
    // person who doesn't know what MESH is is not the person who wants it turned on yet.
    let mut explain = None;
    if let Some(term) = crate::ui::glossary::explains(&e.label) {
        let has_state = e.on.is_some() || e.health.is_some();
        let at = resp.rect.right_center() + vec2(if has_state { -30.0 } else { -12.0 }, 0.0);
        ui.painter().text(
            at,
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::INFO,
            egui::FontId::proportional(13.0),
            Color32::from_gray(150),
        );
        let hit = egui::Rect::from_center_size(at, vec2(18.0, ROW_H));
        if clicked
            && ui
                .ctx()
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| hit.contains(p))
        {
            clicked = false;
            explain = Some(term);
        }
    }
    // Network health replaces the ordinary on-dot. Age and errors live in the click popup instead
    // of making every row carry a miniature status report.
    if let Some(health) = &e.health {
        let state = health.state();
        let (_, color) = health_look(state);
        let dot = resp.rect.right_center() + vec2(-10.0, 0.0);
        ui.painter().circle_filled(dot, 3.5, color);
        let hit_rect = egui::Rect::from_min_max(
            egui::pos2(resp.rect.right() - 24.0, resp.rect.top()),
            resp.rect.right_bottom(),
        );
        let health_resp = ui
            .interact(hit_rect, resp.id.with("health"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Source freshness — click for details");
        if health_resp.clicked() {
            clicked = false;
        }
        egui::Popup::menu(&health_resp).show(|ui| health_popup(ui, health));
    } else if on {
        // Only enabled rows need a state dot; gray dots on every disabled row were visual noise.
        ui.painter()
            .circle_filled(resp.rect.right_center() + vec2(-10.0, 0.0), 3.5, accent);
    }
    Hit {
        clicked,
        explain,
        toggle_favorite,
        resp,
    }
}

/// Registry selection also describes tools and layouts; those are not visible map layers.
fn active_layer(e: &PaletteEntry) -> bool {
    use crate::app::{ContourKind, OverlayToggle as T};
    e.on == Some(true)
        && match e.action {
            PaletteAction::SetMoment(..) | PaletteAction::ToggleField(_) => true,
            PaletteAction::SetContours(kind) => kind != ContourKind::Off,
            PaletteAction::ToggleOverlay(toggle) => {
                !matches!(toggle, T::AlertPanel | T::LinkCameras | T::LinkTimes | T::MiniLoop)
            }
            _ => false,
        }
}

fn active_row(ui: &mut egui::Ui, e: &PaletteEntry, accent: Color32) -> Option<PaletteAction> {
    use egui_phosphor::regular as ph;
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        let label = e.label.split(" (").next().unwrap_or(&e.label);
        let warning = e
            .health
            .as_ref()
            .filter(|h| h.state() != HealthState::Fresh);
        let controls = if warning.is_some() { 102.0 } else { 52.0 };
        ui.allocate_ui_with_layout(
            vec2((ui.available_width() - controls).max(80.0), 38.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(egui::Label::new(RichText::new(label).size(14.0)).wrap())
                    .on_hover_text(format!("{}\n{}", e.label, e.desc));
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if matches!(e.action, PaletteAction::SetMoment(..)) {
                ui.weak("Current");
            } else if ui
                .add(
                    egui::Button::new(RichText::new(ph::TOGGLE_RIGHT).size(28.0).color(accent))
                        .frame(false)
                        .min_size(vec2(36.0, 32.0)),
                )
                .named_toggle(&format!("Show {}", e.label), true)
                .clicked()
            {
                chosen = Some(match e.action {
                    PaletteAction::SetContours(_) => {
                        PaletteAction::SetContours(crate::app::ContourKind::Off)
                    }
                    action => action,
                });
            }
            if let Some(health) = warning {
                let (label, color) = health_look(health.state());
                let status = ui
                    .small_button(RichText::new(label).size(10.0).color(color))
                    .on_hover_text("Source status — click for details");
                egui::Popup::menu(&status).show(|ui| health_popup(ui, health));
            }
        });
    });
    chosen
}

/// A category is navigation, not a layer toggle. Keep the entire tile keyboard-accessible.
fn category_tile(
    ui: &mut egui::Ui,
    cat: &str,
    active: usize,
    width: f32,
    accent: Color32,
) -> egui::Response {
    let response = ui.add_sized(
        vec2(width, 74.0),
        egui::Button::new("")
            .fill(ui.visuals().faint_bg_color)
            .stroke(Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .corner_radius(12.0),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("{}, {active} active", category_name(cat)),
        )
    });
    let r = response.rect.shrink(12.0);
    let painter = ui.painter();
    painter.text(
        r.left_top(),
        egui::Align2::LEFT_TOP,
        category_glyph(cat),
        egui::FontId::proportional(22.0),
        if ui.visuals().dark_mode {
            Color32::from_rgb(112, 215, 228)
        } else {
            accent
        },
    );
    painter.text(
        r.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{active}  ›"),
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        r.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        category_name(cat),
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Observation actions are grouped without duplicating the registry or losing specialist entries.
fn observation_group(action: PaletteAction) -> u8 {
    use crate::app::OverlayToggle as T;
    match action {
        PaletteAction::ToggleOverlay(
            T::Metar | T::Webcams | T::Spotters | T::Gauges | T::Aqi | T::Tropical,
        ) => 0,
        PaletteAction::ToggleOverlay(T::Pireps | T::Recon | T::Aviation) => 1,
        _ => 2,
    }
}
fn observation_row(ui: &mut egui::Ui, e: &PaletteEntry, accent: Color32) -> bool {
    let title = e.label.split(" (").next().unwrap_or(&e.label);
    let on = e.on == Some(true);
    let r = ui
        .add_sized(
            [ui.available_width(), 66.0],
            egui::Button::new("").corner_radius(10.0),
        )
        .named_toggle(&e.label, on)
        .on_hover_text(e.desc);
    let p = ui.painter();
    let rect = r.rect;
    p.text(
        rect.left_top() + vec2(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(15.0),
        ui.visuals().text_color(),
    );
    let desc = e.desc.split('—').next().unwrap_or(e.desc).trim();
    let galley = p.layout(
        desc.to_string(),
        egui::FontId::proportional(10.0),
        ui.visuals().weak_text_color(),
        (rect.width() - 68.0).max(100.0),
    );
    p.galley(
        rect.left_top() + vec2(12.0, 34.0),
        galley,
        ui.visuals().weak_text_color(),
    );
    p.text(
        rect.right_center() - vec2(12.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        if on {
            egui_phosphor::regular::TOGGLE_RIGHT
        } else {
            egui_phosphor::regular::TOGGLE_LEFT
        },
        egui::FontId::proportional(27.0),
        if on {
            accent
        } else {
            ui.visuals().weak_text_color()
        },
    );
    r.clicked()
}

/// The panel body: search box + categorized rows. Returns the clicked action, if any.
/// `focus_search` grabs the search field this frame (Ctrl+K opens the drawer typing-ready).
/// `pref` is the persisted drag order and is rewritten in place when a row is dropped.
#[allow(clippy::too_many_arguments)] // two call sites, both flat; a params struct buys nothing
pub(crate) fn body(
    ui: &mut egui::Ui,
    entries: &[PaletteEntry],
    query: &mut String,
    accent: Color32,
    max_height: f32,
    selected_day: chrono::NaiveDate,
    focus_search: bool,
    pref: &mut Vec<String>,
    recent: &[String],
    favorites: &mut Vec<String>,
    mut after_radar: impl FnMut(&mut egui::Ui),
) -> Option<PaletteAction> {
    let mut chosen = None;
    let nav_id = ui.make_persistent_id("layer_navigation");
    let (mut active_only, mut category) = ui.ctx().data_mut(|d| {
        d.get_temp::<(bool, Option<String>)>(nav_id)
            .unwrap_or_default()
    });
    if focus_search {
        // Ctrl+K and the ribbon's Search all entry point always search the full suite.
        active_only = false;
        category = None;
    }
    // (dragged label, label it was dropped on) — applied after the loop so the borrow of `pref`
    // doesn't have to live inside the scroll area.
    let mut moved: Option<(String, String)> = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                .size(15.0)
                .color(Color32::from_gray(170)),
        );
        let field = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text(if active_only {
                    "Filter active layers…"
                } else {
                    "Search or type: reflectivity, station KTLX, time 21:30Z…"
                })
                .margin(egui::vec2(8.0, 8.0))
                .desired_width(ui.available_width() - 4.0),
        );
        if focus_search {
            field.request_focus();
        }
        // The Android keyboard shrinks the sheet after focus is granted. Reveal the
        // field again when clipped, without pinning the scroll position while browsing.
        if field.has_focus() && !ui.clip_rect().contains_rect(field.rect) {
            field.scroll_to_me(Some(egui::Align::Center));
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - ui.spacing().item_spacing.x) * 0.5;
        let count = entries.iter().filter(|e| active_layer(e)).count();
        for (label, value) in [
            ("Browse".to_string(), false),
            (format!("Active · {count}"), true),
        ] {
            if ui
                .add_sized(
                    vec2(width, 34.0),
                    egui::Button::new(label)
                        .selected(active_only == value)
                        .corner_radius(9.0),
                )
                .clicked()
            {
                active_only = value;
                category = None;
            }
        }
    });
    ui.add_space(10.0);
    let command = command_entry(query, selected_day);
    let order: Vec<_> = matches(entries, query)
        .into_iter()
        .filter(|i| !active_only || active_layer(&entries[*i]))
        .collect();
    // Enter runs the top-ranked match. Type-and-Enter was the whole point of the command palette
    // this drawer replaced; without it the search box is a filter, not a launcher.
    if !query.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(command) = &command {
            return Some(command.action);
        }
        if let Some(i) = order.first() {
            return Some(entries[*i].action);
        }
    }
    let out = egui::ScrollArea::vertical()
        .max_height(max_height)
        .show(ui, |ui| {
            if order.is_empty() && command.is_none() {
                ui.add_space(8.0);
                ui.weak(if active_only && query.is_empty() {
                    "No active layers."
                } else {
                    "No matches."
                });
                return;
            }
            if active_only && command.is_none() {
                for cat in CATEGORIES {
                    let group: Vec<_> = order
                        .iter()
                        .copied()
                        .filter(|i| entries[*i].category == cat)
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(category_name(cat))
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    for i in group {
                        if let Some(action) = active_row(ui, &entries[i], accent) {
                            chosen = Some(action);
                        }
                    }
                    ui.separator();
                }
                after_radar(ui);
                return;
            }
            if !query.is_empty() {
                // Searching: one flat best-first list — categories only add noise here.
                if let Some(command) = &command {
                    let hit = row(ui, command, accent, false, true, favorites);
                    if hit.clicked {
                        chosen = Some(command.action);
                    }
                    ui.add_space(2.0);
                }
                for i in &order {
                    // No dragging in search results: the order you're looking at is the ranking,
                    // not the list you'd be reordering.
                    let hit = row(ui, &entries[*i], accent, false, true, favorites);
                    if hit.clicked {
                        chosen = Some(entries[*i].action);
                    }
                    if let Some(t) = hit.explain {
                        chosen = Some(PaletteAction::Explain(t));
                    }
                    if let Some(slug) = hit.toggle_favorite {
                        toggle_favorite(favorites, &slug);
                    }
                    ui.add_space(2.0);
                }
                return;
            }
            if category.is_none() {
                // Favorites, above Recent: a deliberate pick belongs ahead of an incidental one.
                let favs = favorite_entries(entries, favorites);
                if !favs.is_empty() {
                    ui.label(
                        RichText::new("FAVORITES")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    for entry in favs {
                        let hit = row(ui, entry, accent, false, false, favorites);
                        if hit.clicked {
                            chosen = Some(entry.action);
                        }
                        if let Some(t) = hit.explain {
                            chosen = Some(PaletteAction::Explain(t));
                        }
                        if let Some(slug) = hit.toggle_favorite {
                            toggle_favorite(favorites, &slug);
                        }
                        ui.add_space(2.0);
                    }
                    ui.add_space(6.0);
                }
                // Recent, above the category grid: the landing screen otherwise starts cold every
                // time, asking the user to re-navigate to whatever they were just looking at.
                let recents = recent_entries(entries, recent);
                if !recents.is_empty() {
                    ui.label(
                        RichText::new("RECENT")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    for entry in recents {
                        let hit = row(ui, entry, accent, false, false, favorites);
                        if hit.clicked {
                            chosen = Some(entry.action);
                        }
                        if let Some(t) = hit.explain {
                            chosen = Some(PaletteAction::Explain(t));
                        }
                        if let Some(slug) = hit.toggle_favorite {
                            toggle_favorite(favorites, &slug);
                        }
                        ui.add_space(2.0);
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                }
                let width = (ui.available_width() - 8.0) * 0.5;
                let categories: Vec<_> = CATEGORIES
                    .into_iter()
                    .filter(|cat| entries.iter().any(|e| e.category == *cat))
                    .collect();
                for pair in categories.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for cat in pair {
                            let active = entries
                                .iter()
                                .filter(|e| e.category == *cat && e.on == Some(true))
                                .count();
                            if category_tile(ui, cat, active, width, accent).clicked() {
                                category = Some((*cat).to_string());
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                if let Some(entry) = entries.iter().find(|e| {
                    e.action == PaletteAction::ToggleOverlay(crate::app::OverlayToggle::Tracks)
                }) {
                    let on = entry.on == Some(true);
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(12.0)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(egui_phosphor::regular::PATH)
                                        .size(24.0)
                                        .color(accent),
                                );
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Storm tracks").size(14.0));
                                    ui.label(
                                        RichText::new("Projected cell movement")
                                            .size(10.0)
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let icon = if on {
                                            egui_phosphor::regular::TOGGLE_RIGHT
                                        } else {
                                            egui_phosphor::regular::TOGGLE_LEFT
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(icon).size(32.0).color(if on {
                                                        accent
                                                    } else {
                                                        ui.visuals().weak_text_color()
                                                    }),
                                                )
                                                .frame(false)
                                                .min_size(vec2(40.0, 36.0)),
                                            )
                                            .named_toggle("Projected storm tracks", on)
                                            .clicked()
                                        {
                                            chosen = Some(entry.action);
                                        }
                                    },
                                );
                            });
                        });
                }
                return;
            }
            let selected = category.clone().unwrap();
            if ui
                .button(format!("‹  {}", category_name(&selected)))
                .clicked()
            {
                category = None;
            }
            ui.add_space(6.0);
            if selected == "Obs" {
                let id = ui.id().with("observation_group");
                let mut group = ui.ctx().data_mut(|d| d.get_temp::<u8>(id).unwrap_or(0));
                if group > 0 && ui.button("‹ Everyday observations").clicked() {
                    group = 0;
                }
                ui.weak(match group {
                    1 => "Aviation & flight data",
                    2 => "Advanced observations",
                    _ => "Everyday observations",
                });
                for i in order.iter().filter(|i| {
                    entries[**i].category == "Obs"
                        && observation_group(entries[**i].action) == group
                }) {
                    if observation_row(ui, &entries[*i], accent) {
                        chosen = Some(entries[*i].action);
                    }
                    ui.add_space(4.0);
                }
                if group == 0 {
                    for (label, g) in [
                        ("Aviation & flight data  ›", 1),
                        ("Advanced observations  ›", 2),
                    ] {
                        if ui
                            .add_sized([ui.available_width(), 48.0], egui::Button::new(label))
                            .clicked()
                        {
                            group = g;
                        }
                    }
                }
                ui.ctx().data_mut(|d| d.insert_temp(id, group));
                return;
            }
            for cat in CATEGORIES.into_iter().filter(|cat| *cat == selected) {
                let mut in_cat: Vec<usize> = order
                    .iter()
                    .copied()
                    .filter(|i| entries[*i].category == cat)
                    .collect();
                if in_cat.is_empty() {
                    continue;
                }
                // Dragged rows first, in the order they were dropped; then everyday entries;
                // then registry order. A row that was never dragged still has a stable place.
                in_cat.sort_by_key(|i| {
                    let dragged = pref.iter().position(|s| *s == entries[*i].label);
                    (dragged.unwrap_or(usize::MAX), !entries[*i].common)
                });
                let seq: Vec<String> = in_cat.iter().map(|i| entries[*i].label.clone()).collect();
                for i in in_cat {
                    let Hit {
                        clicked,
                        explain,
                        toggle_favorite: toggle_fav,
                        resp,
                    } = row(ui, &entries[i], accent, true, false, favorites);
                    if clicked {
                        chosen = Some(entries[i].action);
                    }
                    if let Some(t) = explain {
                        chosen = Some(PaletteAction::Explain(t));
                    }
                    if let Some(slug) = toggle_fav {
                        toggle_favorite(favorites, &slug);
                    }
                    // Insertion line above the row the pointer is over, so a drop lands
                    // where the preview says it will.
                    if resp.dnd_hover_payload::<String>().is_some() {
                        let r = resp.rect;
                        ui.painter()
                            .hline(r.x_range(), r.top() - 1.0, Stroke::new(2.0, accent));
                    }
                    if let Some(drag) = resp.dnd_release_payload::<String>() {
                        moved = Some(((*drag).clone(), entries[i].label.clone()));
                    }
                    ui.add_space(2.0);
                }
                // The knobs for the products right above them, not at the bottom of the panel:
                // a threshold or a forecast hour is read together with the layer it belongs to.
                if cat == "Radar" {
                    ui.add_space(2.0);
                    after_radar(ui);
                    ui.add_space(2.0);
                }
                if let Some((drag, before)) = moved.take() {
                    // Only the category the row was dropped in is rewritten — a cross-category
                    // drag would move a layer out of the group its label says it's in.
                    if seq.contains(&drag) {
                        reorder(pref, &seq, &drag, &before);
                    }
                }
            }
        });
    fade_out_bottom(ui, &out);
    ui.ctx()
        .data_mut(|d| d.insert_temp(nav_id, (active_only, category)));
    chosen
}

/// Fade the last few pixels of the scroll viewport into the card colour when there's more below.
/// The viewport cuts wherever the height budget runs out, which lands mid-row often enough that a
/// half-drawn description ("Specific Differential Pha…") read as a rendering bug. A fade says
/// "keep scrolling" instead.
fn fade_out_bottom(ui: &mut egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<()>) {
    const H: f32 = 22.0;
    let more_below = out.content_size.y > out.inner_rect.height() + 1.0
        && out.state.offset.y + out.inner_rect.height() < out.content_size.y - 1.0;
    if !more_below {
        return;
    }
    let r = out.inner_rect;
    let (cr, cg, cb) = crate::ui::style::CARD_FILL;
    let (clear, solid) = (
        Color32::from_rgba_unmultiplied(cr, cg, cb, 0),
        Color32::from_rgb(cr, cg, cb),
    );
    let mut mesh = egui::Mesh::default();
    for (p, c) in [
        (egui::pos2(r.left(), r.bottom() - H), clear),
        (egui::pos2(r.right(), r.bottom() - H), clear),
        (r.right_bottom(), solid),
        (r.left_bottom(), solid),
    ] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_time_commands_use_the_selected_utc_day() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let command = command_entry("time 21:30Z", day).unwrap();
        assert_eq!(command.label, "Seek timeline to 2026-09-14 21:30 UTC");
        assert_eq!(
            command.action,
            PaletteAction::SeekTime(
                day.and_hms_opt(21, 30, 0).unwrap().and_utc().timestamp()
            )
        );
        assert_eq!(
            command_entry("time now", day).unwrap().action,
            PaletteAction::GoLive
        );
        assert!(command_entry("time 25:99", day).is_none());
        assert_eq!(
            parse_utc_time("2026-09-15T00:15:00+02:00", day)
                .unwrap()
                .to_rfc3339(),
            "2026-09-14T22:15:00+00:00"
        );
    }

    #[test]
    fn station_prefix_limits_search_to_site_actions() {
        let entry = |category| PaletteEntry {
            label: "KTLX Oklahoma City".into(),
            category,
            action: PaletteAction::TogglePanel,
            on: None,
            desc: "",
            common: false,
            key: None,
            health: None,
        };
        let entries = [entry("Radar"), entry("Sites")];
        assert_eq!(matches(&entries, "switch station ktlx"), vec![1]);
        assert_eq!(matches(&entries, "station ktlx"), vec![1]);
    }

    #[test]
    fn active_list_excludes_commands_and_off_contours() {
        use crate::app::{ContourKind, OverlayToggle as T};
        let mut entry = PaletteEntry {
            label: "test".into(),
            category: "Reference",
            action: PaletteAction::ToggleOverlay(T::RadarSites),
            on: Some(true),
            desc: "",
            common: true,
            key: None,
            health: None,
        };
        assert!(active_layer(&entry));
        for action in [
            PaletteAction::SetContours(ContourKind::Off),
            PaletteAction::TogglePanel,
            PaletteAction::SetPanes(1),
            PaletteAction::ToggleOverlay(T::AlertPanel),
            PaletteAction::ToggleOverlay(T::LinkCameras),
            PaletteAction::ToggleOverlay(T::LinkTimes),
            PaletteAction::ToggleOverlay(T::MiniLoop),
        ] {
            entry.action = action;
            assert!(!active_layer(&entry), "{action:?}");
        }
        entry.action = PaletteAction::ToggleOverlay(T::Tracks);
        entry.on = Some(false);
        assert!(!active_layer(&entry));
        entry.on = Some(true);
        assert!(active_layer(&entry));
    }

    #[test]
    fn floating_navigation_keeps_categories_active_layers_and_search_reachable() {
        let entries: Vec<_> = CATEGORIES
            .iter()
            .map(|cat| PaletteEntry {
                label: format!("{cat} layer"),
                category: cat,
                action: PaletteAction::ToggleOverlay(if *cat == "Obs" {
                    crate::app::OverlayToggle::Metar
                } else {
                    crate::app::OverlayToggle::RadarSites
                }),
                on: Some(*cat == "Radar"),
                desc: "",
                common: true,
                key: None,
                health: None,
            })
            .collect();
        let render = |active: bool, category: Option<&str>, search: &str| {
            let ctx = egui::Context::default();
            let mut query = search.to_string();
            let mut pref = Vec::new();
            let mut favorites = Vec::new();
            let mut text = Vec::new();
            for _ in 0..3 {
                let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.set_width(308.0);
                    let id = ui.make_persistent_id("layer_navigation");
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(id, (active, category.map(str::to_string))));
                    body(
                        ui,
                        &entries,
                        &mut query,
                        Color32::WHITE,
                        700.0,
                        chrono::Utc::now().date_naive(),
                        false,
                        &mut pref,
                        &[],
                        &mut favorites,
                        |_| {},
                    );
                });
                text = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            text
        };
        let browse = render(false, None, "");
        for cat in CATEGORIES {
            assert!(
                browse.iter().any(|s| s == category_name(cat)),
                "missing {cat}: {browse:?}"
            );
            assert!(render(false, Some(cat), "")
                .iter()
                .any(|s| s == &format!("{cat} layer")));
        }
        let active = render(true, None, "");
        assert!(active.iter().any(|s| s == "Radar layer"));
        assert!(!active.iter().any(|s| s == "National layer"));
        assert!(render(false, Some("Radar"), "National")
            .iter()
            .any(|s| s.contains("National layer")));
    }

    #[test]
    fn focused_search_scrolls_above_keyboard() {
        let ctx = egui::Context::default();
        let mut query = String::new();
        let mut pref = Vec::new();
        let mut favorites = Vec::new();
        let mut offset = 0.0;
        for frame in 0..12 {
            let height = if frame < 3 { 800.0 } else { 300.0 };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, height),
                )),
                time: Some(frame as f64 / 10.0),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let out = egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(400.0); // product controls above the search field
                    body(
                        ui,
                        &[],
                        &mut query,
                        Color32::WHITE,
                        100.0,
                        chrono::Utc::now().date_naive(),
                        frame == 0,
                        &mut pref,
                        &[],
                        &mut favorites,
                        |_| {},
                    );
                    ui.add_space(200.0);
                });
                offset = out.state.offset.y;
            });
        }
        assert!(
            offset > 150.0,
            "focused search stayed behind the keyboard: {offset}"
        );
    }

    #[test]
    fn note_recent_moves_a_repeat_to_the_front_without_duplicating_it() {
        let mut recent = Vec::new();
        note_recent(&mut recent, "mesh");
        note_recent(&mut recent, "rotation");
        note_recent(&mut recent, "lightning");
        assert_eq!(recent, vec!["lightning", "rotation", "mesh"]);
        // Picking something already on the list moves it up rather than listing it twice.
        note_recent(&mut recent, "mesh");
        assert_eq!(recent, vec!["mesh", "lightning", "rotation"]);
    }

    #[test]
    fn note_recent_caps_at_the_configured_length() {
        let mut recent = Vec::new();
        for slug in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            note_recent(&mut recent, slug);
        }
        assert_eq!(recent.len(), RECENT_LAYERS_CAP);
        // Most-recent-first, oldest fallen off the end.
        assert_eq!(recent, vec!["h", "g", "f", "e", "d", "c"]);
    }

    /// A slug that no longer resolves to a catalog entry — a removed or renamed action — must be
    /// skipped rather than producing a row with nothing behind it.
    #[test]
    fn recent_entries_drops_slugs_that_no_longer_resolve() {
        use crate::render::FieldLayer as FL;
        let entries = [
            PaletteEntry {
                label: "Hail size (MESH)".into(),
                category: "National",
                action: PaletteAction::ToggleField(FL::Mesh),
                on: Some(false),
                desc: "",
                common: true,
                key: None,
                health: None,
            },
            PaletteEntry {
                label: "Rotation tracks".into(),
                category: "National",
                action: PaletteAction::ToggleField(FL::Rotation),
                on: Some(false),
                desc: "",
                common: true,
                key: None,
                health: None,
            },
        ];
        let recent = vec![
            "mesh".to_string(),
            "no-longer-exists".to_string(),
            "rotation".to_string(),
        ];
        let resolved = recent_entries(&entries, &recent);
        let labels: Vec<&str> = resolved.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["Hail size (MESH)", "Rotation tracks"]);
    }

    #[test]
    fn toggle_favorite_adds_then_removes() {
        let mut favorites = Vec::new();
        toggle_favorite(&mut favorites, "mesh");
        assert_eq!(favorites, vec!["mesh".to_string()]);
        toggle_favorite(&mut favorites, "rotation");
        assert_eq!(favorites, vec!["mesh".to_string(), "rotation".to_string()]);
        // Starring something already starred un-stars it, at whatever position it was in — a
        // plain toggle, not a "move to front" like `note_recent`'s recency ordering.
        toggle_favorite(&mut favorites, "mesh");
        assert_eq!(favorites, vec!["rotation".to_string()]);
    }

    /// Same failure mode `recent_entries` guards against: a starred slug that no longer resolves
    /// to a catalog entry (removed or renamed action) must be skipped, not left as a dead row.
    #[test]
    fn favorite_entries_drops_slugs_that_no_longer_resolve() {
        use crate::render::FieldLayer as FL;
        let entries = [
            PaletteEntry {
                label: "Hail size (MESH)".into(),
                category: "National",
                action: PaletteAction::ToggleField(FL::Mesh),
                on: Some(false),
                desc: "",
                common: true,
                key: None,
                health: None,
            },
            PaletteEntry {
                label: "Rotation tracks".into(),
                category: "National",
                action: PaletteAction::ToggleField(FL::Rotation),
                on: Some(false),
                desc: "",
                common: true,
                key: None,
                health: None,
            },
        ];
        let favorites = vec![
            "mesh".to_string(),
            "no-longer-exists".to_string(),
            "rotation".to_string(),
        ];
        let resolved = favorite_entries(&entries, &favorites);
        let labels: Vec<&str> = resolved.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["Hail size (MESH)", "Rotation tracks"]);
    }

    /// The star affordance on a row toggles favorite-status without toggling the layer itself —
    /// the two have to land in genuinely separate hit-areas, or starring something would also
    /// flip it on/off as a side effect nobody asked for.
    #[test]
    fn the_star_toggles_favorite_status_without_toggling_the_layer() {
        use crate::render::FieldLayer as FL;
        let entries = [PaletteEntry {
            label: "Hail size (MESH)".into(),
            category: "National",
            action: PaletteAction::ToggleField(FL::Mesh),
            on: Some(false),
            desc: "Estimated largest hail size",
            common: true,
            key: None,
            health: None,
        }];
        // Seeds the landing screen's RECENT section so the row (and its star) are actually drawn
        // — with both `recent` and `favorites` empty, the entry only shows as an unexpanded
        // category tile, which draws no row at all.
        let recent: Vec<String> = vec!["mesh".to_string()];
        let ctx = egui::Context::default();
        let mut query = String::new();
        let mut pref = Vec::new();
        let mut favorites = Vec::new();
        let mut run = |ctx: &egui::Context, favorites: &mut Vec<String>, events: Vec<egui::Event>| {
            let mut chosen = None;
            let out = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(308.0);
                    chosen = body(
                        ui, &entries, &mut query, Color32::WHITE, 700.0,
                        chrono::Utc::now().date_naive(), false, &mut pref,
                        &recent, favorites, |_| {},
                    );
                },
            );
            (chosen, out)
        };
        let (_, out) = run(&ctx, &mut favorites, vec![]);
        let star_pos = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == egui_phosphor::regular::STAR => {
                    Some(t.pos)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("star glyph not drawn"));
        run(&ctx, &mut favorites, vec![
            egui::Event::PointerMoved(star_pos),
            egui::Event::PointerButton {
                pos: star_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        let (chosen, _) = run(&ctx, &mut favorites, vec![egui::Event::PointerButton {
            pos: star_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(chosen, None, "clicking the star must not also toggle the layer");
        assert_eq!(favorites, vec!["mesh".to_string()]);
    }

    /// End to end through the same `body` the app renders: a recent slug shows up under a
    /// "RECENT" heading on the empty-query landing screen, above the category grid, and clicking
    /// its row returns the same toggle action the category browse path would.
    #[test]
    fn recent_section_renders_above_the_category_grid_and_is_clickable() {
        use crate::render::FieldLayer as FL;
        let entries = [PaletteEntry {
            label: "Hail size (MESH)".into(),
            category: "National",
            action: PaletteAction::ToggleField(FL::Mesh),
            on: Some(false),
            desc: "Estimated largest hail size",
            common: true,
            key: None,
            health: None,
        }];
        let recent = vec!["mesh".to_string()];
        let ctx = egui::Context::default();
        let mut query = String::new();
        let mut pref = Vec::new();
        let mut favorites = Vec::new();
        let mut run = |ctx: &egui::Context, events: Vec<egui::Event>| {
            let mut chosen = None;
            let out = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(308.0);
                    chosen = body(
                        ui, &entries, &mut query, Color32::WHITE, 700.0,
                        chrono::Utc::now().date_naive(), false, &mut pref,
                        &recent, &mut favorites, |_| {},
                    );
                },
            );
            (chosen, out)
        };
        // Warm-up frame: lay out the row and find where its label landed.
        let (_, out) = run(&ctx, vec![]);
        let texts: Vec<(String, egui::Pos2)> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos)),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|(s, _)| s == "RECENT"), "{texts:?}");
        let pos = texts
            .iter()
            .find(|(s, _)| s == "Hail size (MESH)")
            .map(|(_, p)| *p)
            .unwrap_or_else(|| panic!("recent row label not drawn: {texts:?}"));
        // Same click-simulation pattern as `style.rs`'s toggle test: move onto the row, press,
        // then release — `row()` reports a click on release, matching every other button in
        // this UI (a press alone must not fire the action, or a drag-away would still trigger it).
        run(&ctx, vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        let (chosen, _) = run(&ctx, vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(chosen, Some(PaletteAction::ToggleField(FL::Mesh)));
    }

    #[test]
    fn catalog_rows_resolve_saved_layers_and_search_metadata() {
        let entries: Vec<_> = wxdata::mrms::catalog::PRODUCTS
            .iter()
            .map(|product| {
                let layer = crate::render::FieldLayer::from_slug(product.field.id.0)
                    .expect("catalog product must resolve to a saved layer ID");
                assert_eq!(layer.descriptor().unwrap().id, product.field.id);
                PaletteEntry {
                    label: product.field.name.into(),
                    category: "National",
                    action: PaletteAction::ToggleField(layer),
                    on: Some(false),
                    desc: product.field.description,
                    common: product.common,
                    key: None,
                    health: None,
                }
            })
            .collect();
        assert_eq!(matches(&entries, "NOAA MRMS").len(), entries.len());
        for (query, slug) in [
            ("mm/hr", "preciprate"), ("NLDN", "lightning"),
            ("hydrology", "flashflood"), ("gauge corrected", "qpe1h"),
        ] {
            let action = PaletteAction::ToggleField(crate::render::FieldLayer::from_slug(slug).unwrap());
            assert!(
                matches(&entries, query).iter().any(|i| entries[*i].action == action),
                "{query} must find {slug}"
            );
        }
    }

    /// Grouping must never hide a row for good: every specialist entry remains searchable.
    #[test]
    fn every_entry_is_reachable_from_the_registry() {
        let entries = [
            PaletteEntry {
                label: "Echo tops (L3)".into(),
                category: "National",
                action: PaletteAction::CycleBasemap,
                on: None,
                desc: "How tall the storm is",
                common: false,
                key: None,
                health: None,
            },
            PaletteEntry {
                label: "MRMS Mosaic".into(),
                category: "National",
                action: PaletteAction::CycleBasemap,
                on: None,
                desc: "Every radar stitched together",
                common: true,
                key: None,
                health: None,
            },
        ];
        // Empty query = the full list, common or not.
        assert_eq!(matches(&entries, "").len(), entries.len());
        // And an uncommon row is still findable by name.
        assert_eq!(matches(&entries, "echo"), vec![0]);
    }

    /// The drawer's Enter key runs `matches(...)[0]`, so the ranking has to put the obvious
    /// answer first for the labels people actually type.
    #[test]
    fn top_match_is_the_obvious_one() {
        let e = |label: &str| PaletteEntry {
            label: label.into(),
            category: "Radar",
            action: PaletteAction::CycleBasemap,
            on: None,
            desc: "",
            common: true,
            key: None,
            health: None,
        };
        let entries = [
            e("Storm-Relative Velocity"),
            e("Velocity"),
            e("Reflectivity"),
        ];
        assert_eq!(matches(&entries, "velocity").first(), Some(&1));
        assert_eq!(matches(&entries, "refl").first(), Some(&2));
    }

    #[test]
    fn dropping_a_row_puts_it_where_the_preview_said() {
        let seq: Vec<String> = ["Reflectivity", "Velocity", "Spectrum Width"]
            .map(String::from)
            .to_vec();
        let mut pref = vec!["Some other category's row".to_string()];
        // Drop Spectrum Width onto Velocity: it lands *above* Velocity, matching the line drawn
        // along the hovered row's top edge.
        reorder(&mut pref, &seq, "Spectrum Width", "Velocity");
        assert_eq!(
            pref,
            [
                "Some other category's row",
                "Reflectivity",
                "Spectrum Width",
                "Velocity",
            ]
        );
        // A second drag rewrites the same labels rather than appending them twice.
        let seq2: Vec<String> = ["Reflectivity", "Spectrum Width", "Velocity"]
            .map(String::from)
            .to_vec();
        reorder(&mut pref, &seq2, "Velocity", "Reflectivity");
        assert_eq!(
            pref,
            [
                "Some other category's row",
                "Velocity",
                "Reflectivity",
                "Spectrum Width",
            ]
        );
        // Dropping a row on itself is a no-op, not a reshuffle.
        let before = pref.clone();
        reorder(&mut pref, &seq2, "Velocity", "Velocity");
        assert_eq!(pref, before);
    }

    #[test]
    fn fuzzy_subsequence_and_ranking() {
        assert!(fuzzy("vel", "Velocity").is_some());
        assert!(fuzzy("srv", "Storm-Relative Velocity").is_some());
        assert!(fuzzy("gau", "River gauges (NWPS)").is_some());
        assert!(fuzzy("zzz", "Velocity").is_none());
        // Empty query matches everything.
        assert_eq!(fuzzy("", "anything"), Some(0));
        // A tighter (contiguous) match must rank ahead of a scattered one.
        let tight = fuzzy("cape", "CAPE").unwrap();
        let loose = fuzzy("cape", "Cell arrival probability estimate").unwrap();
        assert!(tight < loose, "tight {tight} should beat loose {loose}");
    }
}
