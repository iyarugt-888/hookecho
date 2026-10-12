//! A field picked in "Model fields", at one point over its run (ROADMAP_NEW F4 "time series",
//! 1010.md V16). From the layer probe's pinned card, "Graph this point over the run" fetches the
//! same message (variable, level, timing) at every lead of the run the pane shows, pinned to that
//! one run, and graphs its value at the point in the field's display units. A lead the run does
//! not have is a gap in the line, not an error for the whole graph. Regional models only: the
//! forecast window already graphs the global models.

use super::*;
use chrono::Timelike;

/// Leads fetched at once.
const AT_ONCE: usize = 4;

type SeriesReply = Result<(DateTime<Utc>, Vec<(DateTime<Utc>, Option<f32>)>), String>;

#[derive(Default)]
pub(crate) struct FieldSeries {
    pub open: bool,
    /// The field, model and level, and the point.
    pub title: String,
    pub unit: String,
    pub point: [f64; 2],
    /// The run the series is from, once known.
    pub run: Option<DateTime<Utc>>,
    pub state: Option<ui::forecast_window::SeriesState>,
    rx: Option<std::sync::mpsc::Receiver<SeriesReply>>,
}

/// How many decimals a series in `unit` is labelled with.
fn decimals(unit: &str) -> usize {
    match unit {
        "g/kg" | "in" | "mm/h" => 1,
        _ => 0,
    }
}

impl HookEchoApp {
    /// Graph pane `idx`'s browsed field at `point` over the run it shows.
    pub(crate) fn start_field_series(&mut self, idx: usize, point: [f64; 2], ctx: &egui::Context) {
        use super::model_field::InventorySource;
        let s = &mut self.field_series;
        s.open = true;
        s.point = point;
        s.run = None;
        s.rx = None;
        let Some(ModelRequest::Discovered(pick, _, run)) =
            self.selected_model_request_for(idx, crate::render::FieldLayer::ModelField)
        else {
            self.field_series.state = Some(ui::forecast_window::SeriesState::Failed(
                "No model field is picked on this pane".into(),
            ));
            return;
        };
        let InventorySource::Regional(model) = pick.model else {
            self.field_series.state = Some(ui::forecast_window::SeriesState::Failed(
                "Point graphs here are for the regional models; the forecast window graphs the \
                 global ones"
                    .into(),
            ));
            return;
        };
        let quantity = wxdata::model_inventory::vetted(pick.var);
        let s = &mut self.field_series;
        s.unit = quantity.map_or_else(String::new, |q| q.unit.to_string());
        s.title = format!(
            "{} {}, {}",
            model.label(),
            quantity.map_or(pick.var, |q| q.name),
            pick.level
        );
        s.state = Some(ui::forecast_window::SeriesState::Loading);
        let (tx, rx) = std::sync::mpsc::channel();
        s.rx = Some(rx);
        let http = self.http.clone();
        let ctx = ctx.clone();
        let (var, level, kind) = (pick.var, pick.level, pick.kind);
        self.spawner.spawn(async move {
            let reply: SeriesReply = async {
                // The first lead pins the run every later lead is read from.
                let first =
                    wxdata::hrrr::fetch_inventory_field(&http, model, run, 0, var, level, kind)
                        .await
                        .map_err(|e| format!("{e:#}"))?;
                let run = first.run;
                let max = model.max_lead_for_cycle(run.hour()).min(u16::from(u8::MAX)) as u8;
                let mut out = vec![(
                    first.valid(),
                    first.field.sample_bilinear(point[0], point[1]),
                )];
                let leads: Vec<u8> = (1..=max).collect();
                for group in leads.chunks(AT_ONCE) {
                    let jobs = group.iter().map(|&lead| {
                        let http = http.clone();
                        async move {
                            let valid = run + chrono::Duration::hours(i64::from(lead));
                            let value = wxdata::hrrr::fetch_inventory_field(
                                &http,
                                model,
                                Some(run),
                                lead,
                                var,
                                level,
                                kind,
                            )
                            .await
                            .ok()
                            .and_then(|f| f.field.sample_bilinear(point[0], point[1]));
                            (valid, value)
                        }
                    });
                    out.extend(futures_util::future::join_all(jobs).await);
                    ctx.request_repaint();
                }
                Ok((run, out))
            }
            .await;
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }

    /// The point graph's window, when one was asked for.
    pub(crate) fn field_series_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.field_series.rx {
            if let Ok(reply) = rx.try_recv() {
                self.field_series.rx = None;
                self.field_series.state = Some(match reply {
                    Ok((run, points)) => {
                        self.field_series.run = Some(run);
                        ui::forecast_window::SeriesState::Ready(points)
                    }
                    Err(e) => ui::forecast_window::SeriesState::Failed(e),
                });
            }
        }
        if !self.field_series.open {
            return;
        }
        let tz = self.active_tz();
        let s = &mut self.field_series;
        let mut open = true;
        egui::Window::new("Model field at a point")
            .id(egui::Id::new("field_series"))
            .open(&mut open)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.strong(&s.title);
                ui.weak(format!(
                    "{:.3}, {:.3}{}{}",
                    s.point[1],
                    s.point[0],
                    if s.unit.is_empty() {
                        String::new()
                    } else {
                        format!(" \u{b7} {}", s.unit)
                    },
                    s.run.map_or_else(String::new, |r| format!(
                        " \u{b7} run {}",
                        r.format("%Y-%m-%d %HZ")
                    ))
                ));
                match &s.state {
                    None | Some(ui::forecast_window::SeriesState::Idle) => {}
                    Some(ui::forecast_window::SeriesState::Loading) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Fetching every lead of the run\u{2026}");
                        });
                    }
                    Some(ui::forecast_window::SeriesState::Failed(e)) => {
                        ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e);
                    }
                    Some(ui::forecast_window::SeriesState::Ready(points)) => {
                        ui::forecast_window::paint_series(ui, points, decimals(&s.unit), tz);
                        let missing = points.iter().filter(|(_, v)| v.is_none()).count();
                        ui.weak(format!(
                            "{} leads, one run{}",
                            points.len(),
                            if missing > 0 {
                                format!("; {missing} without a value here (gaps in the line)")
                            } else {
                                String::new()
                            }
                        ));
                    }
                }
            });
        if !open {
            self.field_series.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_with_small_units_keep_a_decimal() {
        assert_eq!(decimals("g/kg"), 1);
        assert_eq!(decimals("°C"), 0);
        assert_eq!(decimals("kt"), 0);
    }
}
