//! The layer probe: every visible layer's reading under the pointer, in one card. The radar
//! moment (or the user product shown in its place), each gridded field on the pane (MRMS, model,
//! satellite, rotation tracks...), each model contour (STP, CAPE, SRH...), the rotation and debris
//! tracks and storm cells near the point, and the alerts, watches, discussions and outlook over
//! it. A click pins the card to that point; another click moves it; the card's own button lets go.
//!
//! The J3 cursor probe (`ui::cursor_probe`) answers a different question — one pane's top layer,
//! compared across linked panes — and stays as it is.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::{ContourKind, HookEchoApp};
use wxdata::level2::Moment;
use wxdata::overlay::FeatureKind;

#[path = "layer_probe_ui.rs"]
mod presentation;

/// Storm cells and detection tracks this close to the point are listed.
const NEAR_KM: f64 = 10.0;

/// One layer's reading at the point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeLine {
    /// What it is: "REF 0.5°", "MRMS rotation track 60 min", "STP (fixed)".
    pub layer: String,
    /// Its reading, in the layer's own units; "—" where it has none here.
    pub value: String,
    /// Source, time or other context, when there is some.
    pub detail: Option<String>,
    pub stamp: Option<wxdata::field::DataStamp>,
    pub field: Option<crate::render::FieldLayer>,
}

impl ProbeLine {
    fn new(layer: impl Into<String>, value: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            layer: layer.into(),
            value: value.into(),
            detail,
            stamp: None,
            field: None,
        }
    }
}

/// The display units a contour's grid is kept in (`ContourKind::to_display`).
fn contour_units(kind: ContourKind, temp_unit: crate::settings::TempUnit) -> &'static str {
    match kind {
        ContourKind::Mslp => "hPa",
        ContourKind::T2m | ContourKind::Td2m => temp_unit.label(),
        ContourKind::Cape => "J/kg",
        ContourKind::Srh | ContourKind::EffSrh => "m\u{b2}/s\u{b2}",
        ContourKind::EffShear => "kt",
        ContourKind::Lapse700500 | ContourKind::Lapse850500 => "\u{b0}C/km",
        _ => "",
    }
}

/// A number with as many decimals as its size deserves: 2350 J/kg, 1.4 STP, 0.35.
fn fmt_value(v: f32) -> String {
    let a = v.abs();
    if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// The nearest point of any track within [`NEAR_KM`]: `(km, confidence, time)`.
fn nearest_track_point(
    tracks: &[wxdata::scoretrack::ScoreTrack],
    lon: f64,
    lat: f64,
) -> Option<(f64, f32, chrono::DateTime<chrono::Utc>)> {
    tracks
        .iter()
        .flat_map(|t| t.points.iter())
        .map(|p| {
            let km = crate::geo::great_circle([lon, lat], [p.lon, p.lat]).0;
            (km, p.confidence, p.time)
        })
        .filter(|(km, ..)| *km <= NEAR_KM)
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// A source's signed offset from the time it is read against: " (Δ-42s vs radar)". Positive is
/// after the reference; the reference is named so the sign means something.
pub(crate) fn offset_note(
    t: chrono::DateTime<chrono::Utc>,
    reference: (chrono::DateTime<chrono::Utc>, &str),
) -> String {
    format!(
        " (\u{394}{} vs {})",
        crate::ui::data_inspector::offset_label(t - reference.0),
        reference.1
    )
}

impl HookEchoApp {
    /// Every visible layer's reading on pane `idx` at `(lon, lat)`, radar first.
    pub(crate) fn layer_probe_lines(&mut self, idx: usize, lon: f64, lat: f64) -> Vec<ProbeLine> {
        let mut out = Vec::new();
        let tz = self.active_tz();
        let clock = |t: chrono::DateTime<chrono::Utc>| crate::timefmt::fmt_clock(t, tz, false);
        // What each source's time is measured against (ROADMAP_2 §10.2): the linked analysis
        // time when panes share one, else this pane's radar scan.
        let linked = self.linked_analysis_time();
        let reference = linked
            .map(|t| (t, "analysis"))
            .or_else(|| self.views[idx].displayed_radar_time().map(|t| (t, "radar")));

        // The radar: the user product in the moment's place, else the moment on the shown tilt.
        let product = self.map_product(idx);
        let product_name = self.views[idx].user_product.clone();
        let dealias = self.settings.dealias_velocity;
        let storm_uv = self.views[idx].storm_motion_uv();
        let velocity_unit = self.settings.velocity_unit;
        let v = &mut self.views[idx];
        let (moment, tilt, srv) = (v.moment, v.tilt, v.srv);
        let site = v.site.clone();
        let dealias = dealias
            && moment == Moment::Velocity
            && !site.as_deref().is_some_and(wxdata::tdwr::is_tdwr);
        if let Some(vol) = v.volume.as_mut().filter(|vol| !vol.elevations.is_empty()) {
            let elev = vol.elevations.get(tilt).copied().unwrap_or(0.0);
            let scan_time = vol.acquisition_time(moment, tilt);
            let (label, sweep) = match &product {
                Some((spec, key)) => (
                    format!(
                        "{} {elev:.1}\u{b0}",
                        product_name.clone().unwrap_or_default()
                    ),
                    vol.product_sweep(*key, &spec.expr, spec.range, spec.env, tilt),
                ),
                None => (
                    format!("{} {elev:.1}\u{b0}", crate::products::name(moment, srv)),
                    vol.binned(moment, tilt, dealias),
                ),
            };
            let units = match &product_name {
                Some(name) => self
                    .settings
                    .udp_products
                    .iter()
                    .find(|p| &p.name == name)
                    .map(|p| p.units.clone())
                    .unwrap_or_default(),
                None => moment.units().to_string(),
            };
            if let Ok(s) = sweep {
                let sample = s.sample_at(lon, lat);
                let beam_kft = sample
                    .as_ref()
                    .map(|g| s.beam_height_ft(g.range_km) / 1000.0);
                let value = match &sample {
                    Some(g) if g.folded => "Range folded".to_string(),
                    Some(g) => if product.is_some() {
                        g.value.map(|x| format!("{} {units}", fmt_value(x)))
                    } else {
                        super::radar_probe::format_value(
                            moment,
                            super::radar_probe::relative_value(
                                moment,
                                g.value,
                                g.azimuth_deg,
                                storm_uv,
                            ),
                            velocity_unit,
                        )
                    }
                    .unwrap_or_else(|| "\u{2014}".into()),
                    None => "Outside the sweep".into(),
                };
                let mut detail = match (&site, beam_kft) {
                    (Some(site), Some(kft)) => Some(format!("{site}, beam {kft:.1} kft")),
                    (Some(site), None) => Some(site.clone()),
                    _ => Some("Radar".into()),
                };
                let d = detail.get_or_insert_with(String::new);
                d.push_str(&format!(", tilt acquired {}", scan_time));
                if product.is_none() {
                    if dealias {
                        d.push_str(", dealiased");
                    }
                    if storm_uv.is_some() {
                        d.push_str(", storm motion subtracted");
                    }
                    let (_, displayed) = super::radar_probe::display_units(moment, velocity_unit);
                    if displayed != moment.units() {
                        d.push_str(&format!(", native {} → {displayed}", moment.units()));
                    }
                }
                if let Some(time) = sample
                    .as_ref()
                    .and_then(|g| g.collected_ms)
                    .and_then(chrono::DateTime::from_timestamp_millis)
                {
                    d.push_str(&format!(", sampled radial {time}"));
                }
                // Against a linked analysis time only: against its own scan it is always zero.
                if let (Some(d), Some(analysis)) = (detail.as_mut(), linked) {
                    d.push_str(&offset_note(scan_time, (analysis, "analysis")));
                }
                // A stamp only for the live head, whose receipt the live scan recorded.
                let live_head = v.timeline.following && !v.timeline.playing;
                let stamp = site.as_deref().filter(|_| live_head).and_then(|site| {
                    super::radar_probe::radar_stamp(
                        site,
                        &label,
                        v.live_scan.provider.as_deref(),
                        scan_time,
                        v.live_scan.last_received,
                        product.is_some()
                            || srv && moment == Moment::Velocity
                            || moment == Moment::SpecificDifferentialPhase,
                    )
                });
                let mut line = ProbeLine::new(label, value, detail);
                line.stamp = stamp;
                out.push(line);
            }
        }

        // Every gridded field on the pane, top of the draw order first.
        let fields: Vec<_> = crate::render::FieldLayer::DRAW_ORDER
            .iter()
            .rev()
            .copied()
            .filter(|l| self.views[idx].fields_on.contains(l))
            .collect();
        for layer in fields {
            let row = self.grid_probe_row(idx, layer, lon, lat);
            let detail = match (row.time, reference) {
                (Some(t), Some(r)) => format!("{}, {}{}", row.source, clock(t), offset_note(t, r)),
                (Some(t), None) => format!("{}, {}", row.source, clock(t)),
                (None, _) => row.source.clone(),
            };
            let mut line = ProbeLine::new(
                row.product,
                row.value.unwrap_or_else(|| "\u{2014}".into()),
                Some(detail),
            );
            line.stamp = self
                .fields
                .get(&layer)
                .and_then(|state| state.stamp.clone());
            line.field = Some(layer);
            out.push(line);
        }

        // Model contours: the grid each is drawn from.
        let temp_unit = self.settings.temp_unit;
        for (kind, entry) in &self.contours {
            let Some(grid) = entry.grid.as_ref() else {
                continue;
            };
            let value = grid
                .sample_bilinear(lon, lat)
                .filter(|v| v.is_finite())
                .map(|v| format!("{} {}", fmt_value(v), contour_units(*kind, temp_unit)))
                .unwrap_or_else(|| "\u{2014}".into());
            out.push(ProbeLine::new(
                format!("{} (contours)", kind.label()),
                value.trim_end().to_string(),
                entry.valid.map(|t| {
                    let note = reference.map(|r| offset_note(t, r)).unwrap_or_default();
                    format!("{}, valid {}{note}", self.env_model.label(), clock(t))
                }),
            ));
        }

        // Rotation and debris tracks near the point, when they are drawn.
        let tracks = [
            (
                self.filters.show_couplets,
                self.rot_tracks_cache.as_ref(),
                "Rotation track",
            ),
            (
                self.filters.show_tds,
                self.tds_tracks_cache.as_ref(),
                "Debris (TDS) track",
            ),
        ];
        for (on, cache, name) in tracks {
            let Some((_, tracks)) = cache.filter(|((pane, ..), _)| on && *pane == idx) else {
                continue;
            };
            if let Some((km, conf, t)) = nearest_track_point(tracks, lon, lat) {
                out.push(ProbeLine::new(
                    name,
                    format!("{:.0}% confidence", conf * 100.0),
                    Some(format!("{km:.1} km away, {}", clock(t))),
                ));
            }
        }

        // The nearest storm cell.
        if self.filters.show_cells {
            if let Some((km, c)) = self
                .active_storm_cells()
                .iter()
                .filter(|c| c.kind == wxdata::level3::CellKind::Storm)
                .map(|c| (crate::geo::great_circle([lon, lat], [c.lon, c.lat]).0, c))
                .filter(|(km, _)| *km <= NEAR_KM)
                .min_by(|a, b| a.0.total_cmp(&b.0))
            {
                let mut v = Vec::new();
                if let Some(d) = c.max_dbz {
                    v.push(format!("{d:.0} dBZ"));
                }
                if let Some(t) = c.top_kft {
                    v.push(format!("top {t:.0} kft"));
                }
                if let Some(h) = c.hail_in.filter(|h| *h > 0.0) {
                    v.push(format!("hail {h:.2} in"));
                }
                if c.tvs.is_some() {
                    v.push("TVS".into());
                } else if c.meso.is_some() {
                    v.push("meso".into());
                }
                let motion = match (c.mvt_deg, c.mvt_kt) {
                    (Some(d), Some(k)) => format!(", moving toward {d:.0}\u{b0} at {k:.0} kt"),
                    _ => String::new(),
                };
                out.push(ProbeLine::new(
                    format!("Storm {}", c.id),
                    v.join(", "),
                    Some(format!("{km:.1} km away{motion}")),
                ));
            }
        }

        // Alerts, watches, discussions and the outlook over the point.
        // The live alert feed's receipt, for the alerts' stamps; archived warnings shown on a
        // replay have none recorded, so they keep the "stamp unavailable" line.
        let alert_feed = (self.arch_warn_shown.is_none()).then(|| {
            let h = self.request_health(crate::app::RequestLane::Feed(
                crate::source_health::FeedSource::WeatherAlerts,
            ));
            let source = format!("{} ({})", h.source, h.endpoint_family.label());
            let received = h.last_success.and_then(|age| {
                chrono::Duration::from_std(age)
                    .ok()
                    .map(|age| chrono::Utc::now() - age)
            });
            (source, received)
        });
        let mut seen: Vec<String> = Vec::new();
        let mut areas: Vec<ProbeLine> = Vec::new();
        {
            let mut add = |f: &wxdata::overlay::GeoFeature, what: &str| {
                if !f.contains(lon, lat) {
                    return;
                }
                let id = f
                    .alert
                    .as_ref()
                    .map(|a| a.id.clone())
                    .unwrap_or_else(|| f.title.clone());
                if seen.contains(&id) {
                    return;
                }
                seen.push(id);
                let until = f
                    .alert
                    .as_ref()
                    .and_then(|a| a.expires)
                    .map(|t| format!("until {}", clock(t)));
                let mut line = ProbeLine::new(what, f.title.clone(), until);
                line.stamp = match (&alert_feed, &f.alert) {
                    (Some((source, received)), Some(a)) => alert_stamp(a, source, *received),
                    _ => None,
                };
                areas.push(line);
            };
            if self.filters.show_alerts {
                for f in self.active_alert_features() {
                    let what = match f.kind {
                        FeatureKind::Warning => "Warning",
                        FeatureKind::Watch | FeatureKind::WatchBox => "Watch",
                        FeatureKind::Advisory => "Advisory",
                        FeatureKind::Statement => "Statement",
                        _ => continue,
                    };
                    add(f, what);
                }
            }
            if self.filters.show_watches {
                for f in &self.watch_features {
                    add(f, "Watch");
                }
            }
            if self.filters.show_mds {
                for f in &self.md_features {
                    add(f, "Discussion");
                }
            }
            let day = self.filters.outlook_day;
            if (1..=8).contains(&day) {
                for f in &self.outlook_features[(day - 1) as usize] {
                    add(f, "Outlook");
                }
            }
        }
        out.extend(areas);
        out
    }

    /// While the layer probe is on: a card of [`Self::layer_probe_lines`] beside the pointer on
    /// the pane under it, or at the pinned point. A click on the map pins it there.
    pub(crate) fn paint_layer_probe(&mut self, ui: &egui::Ui, rects: &[egui::Rect]) {
        if !self.settings.layer_probe {
            self.layer_probe_pin = None;
            return;
        }
        let ctx = ui.ctx().clone();
        let (hover, clicked) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked()));
        // Only the map itself: a window or card over it keeps its clicks.
        let on_map = |p: egui::Pos2| {
            ctx.layer_id_at(p)
                .is_none_or(|l| l.order == egui::Order::Background)
        };
        let to_lonlat = |idx: usize, rect: &egui::Rect, p: egui::Pos2| {
            let w = self.views[idx].camera.screen_to_world(
                (p.x - rect.left(), p.y - rect.top()),
                (rect.width(), rect.height()),
            );
            crate::render::mercator::world_to_lonlat(w.0, w.1)
        };
        let hovered = hover.filter(|p| on_map(*p)).and_then(|p| {
            rects
                .iter()
                .position(|r| r.contains(p))
                .map(|idx| (idx, to_lonlat(idx, &rects[idx], p)))
        });
        if clicked {
            if let Some((idx, (lon, lat))) = hovered {
                self.layer_probe_pin = Some((idx, lon, lat));
            }
        }
        let (idx, (lon, lat), pinned) = match (self.layer_probe_pin, hovered) {
            (Some((idx, lon, lat)), _) if idx < rects.len() => (idx, (lon, lat), true),
            (_, Some((idx, at))) => (idx, at, false),
            _ => return,
        };
        let rect = rects[idx];
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let s = self.views[idx]
            .camera
            .world_to_screen(world, (rect.width(), rect.height()));
        let at = egui::pos2(rect.left() + s.0, rect.top() + s.1);
        let accent = crate::theme::accent(self.settings.theme);
        if pinned && rect.contains(at) {
            let painter = ui.painter_at(rect);
            painter.circle_stroke(at, 7.0, egui::Stroke::new(3.0, egui::Color32::BLACK));
            painter.circle_stroke(at, 7.0, egui::Stroke::new(1.5, accent));
            painter.circle_filled(at, 2.0, accent);
        }
        let lines = self.layer_probe_lines(idx, lon, lat);
        let mut unpin = false;
        // Beside the point, on whichever side has room: left of it near the right edge, above it
        // in the lower half (where the timeline sits).
        let card_w = presentation::card_width(rect.width());
        let left = at.x + 18.0 + card_w > rect.right();
        let above = at.y > rect.center().y;
        let pivot = egui::Align2([
            if left {
                egui::Align::Max
            } else {
                egui::Align::Min
            },
            if above {
                egui::Align::Max
            } else {
                egui::Align::Min
            },
        ]);
        let pos = at
            + egui::vec2(
                if left { -18.0 } else { 18.0 },
                if above { -14.0 } else { 14.0 },
            );
        let tokens = self.ws_tokens();
        let analysis_time = self
            .linked_analysis_time()
            .or_else(|| self.views[idx].displayed_radar_time());
        let tolerance = chrono::Duration::minutes(i64::from(self.settings.time_mismatch_minutes));
        egui::Area::new(egui::Id::new("layer_probe"))
            .order(egui::Order::Foreground)
            .pivot(pivot)
            .fixed_pos(pos)
            .constrain_to(rect)
            .interactable(pinned)
            .show(&ctx, |ui| {
                crate::ui::workstation::card_frame(&tokens)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        crate::ui::workstation::style_scope(ui, &tokens);
                        unpin = presentation::show(
                            ui,
                            &lines,
                            pinned,
                            [lon, lat],
                            card_w,
                            rect.height(),
                            analysis_time,
                            tolerance,
                        );
                    });
            });
        if unpin {
            self.layer_probe_pin = None;
        }
    }
}

/// An alert's stamp for the probe's source inspector (ROADMAP_2 §9.1): the product, when it
/// was sent and takes effect, and when this app received it. Hazard products are forecasts.
/// `None` without a known receipt or any time to call it valid from.
fn alert_stamp(
    a: &wxdata::overlay::AlertInfo,
    source: &str,
    received: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<wxdata::field::DataStamp> {
    Some(wxdata::field::DataStamp {
        source_id: source.to_string(),
        product_id: a.event.clone(),
        issue_time: a.issued,
        run_time: None,
        valid_time: a.effective.or(a.issued)?,
        received_time: received?,
        source_latency: None,
        is_forecast: true,
        is_derived: false,
        quality: wxdata::field::QualitySummary::Unknown,
        grid: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alert_is_stamped_from_its_own_times_and_the_feed_receipt() {
        let t = |m: i64| chrono::DateTime::from_timestamp(m * 60, 0).unwrap();
        let a = wxdata::spoken::demo_alert();
        let a = wxdata::overlay::AlertInfo {
            issued: Some(t(100)),
            effective: Some(t(101)),
            ..a
        };
        let s = alert_stamp(&a, "Weather alerts (NWS)", Some(t(102))).unwrap();
        assert_eq!(
            (s.issue_time, s.valid_time, s.received_time),
            (Some(t(100)), t(101), t(102))
        );
        assert!(s.is_forecast && !s.is_derived);
        // No effective time: valid from when it was sent.
        let sent_only = wxdata::overlay::AlertInfo {
            effective: None,
            ..a.clone()
        };
        assert_eq!(
            alert_stamp(&sent_only, "x", Some(t(102)))
                .unwrap()
                .valid_time,
            t(100)
        );
        // No receipt known, or no time at all: no stamp, not a guessed one.
        assert!(alert_stamp(&a, "x", None).is_none());
        let timeless = wxdata::overlay::AlertInfo {
            issued: None,
            effective: None,
            ..a
        };
        assert!(alert_stamp(&timeless, "x", Some(t(102))).is_none());
    }

    #[test]
    fn a_source_offset_is_signed_against_a_named_reference() {
        use chrono::TimeZone;
        let radar = chrono::Utc.with_ymd_and_hms(2026, 5, 6, 21, 0, 0).unwrap();
        let goes = radar - chrono::Duration::seconds(42);
        let mrms = radar + chrono::Duration::seconds(75);
        assert_eq!(
            offset_note(goes, (radar, "radar")),
            " (\u{394}-42s vs radar)"
        );
        assert_eq!(
            offset_note(mrms, (radar, "radar")),
            " (\u{394}+1m 15s vs radar)"
        );
    }

    #[test]
    fn values_keep_the_decimals_their_size_deserves() {
        assert_eq!(fmt_value(2350.4), "2350");
        assert_eq!(fmt_value(42.46), "42.5");
        assert_eq!(fmt_value(1.414), "1.41");
        assert_eq!(fmt_value(-0.35), "-0.35");
    }

    #[test]
    fn the_nearest_track_point_within_reach_is_the_one_read() {
        let t = chrono::Utc::now();
        let pt = |lon: f64, confidence: f32| wxdata::scoretrack::ScorePoint {
            lon,
            lat: 35.0,
            confidence,
            time: t,
        };
        let tracks = vec![wxdata::scoretrack::ScoreTrack {
            points: vec![pt(-97.05, 0.4), pt(-97.01, 0.9), pt(-96.0, 1.0)],
        }];
        let (km, conf, _) = nearest_track_point(&tracks, -97.0, 35.0).unwrap();
        assert!(km < 2.0 && (conf - 0.9).abs() < 1e-6);
        assert!(
            nearest_track_point(&tracks, -95.0, 35.0).is_none(),
            "too far"
        );
    }
}
