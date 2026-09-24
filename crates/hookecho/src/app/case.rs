//! Saving and opening case-study packages (ROADMAP_NEW K3). The file format and its rules live in
//! [`crate::case`]; this is the app side: what goes into one from the current session, and what
//! opening one does to it.

use super::{HookEchoApp, Stroke2d, ToastKind};
use crate::case::{merge_by, CaseManifest, CaseStroke, FORMAT};

impl HookEchoApp {
    /// The current session as a case: the pane arrangement, the active pane's analysis time and
    /// replay window, and every bookmark, marker, zone, stroke and user-defined product.
    pub(crate) fn capture_case(&mut self) -> CaseManifest {
        let v = &self.views[self.active];
        let time = (!v.timeline.following)
            .then(|| v.timeline.current().and_then(|id| id.date_time()))
            .flatten();
        // A case is for replaying an event, so an archive case with no window of its own gets an
        // hour around its instant; a live one has nothing to replay.
        let span_min = match (time, v.timeline.replay_span_min) {
            (None, _) => 0,
            (Some(_), 0) => 60,
            (Some(_), s) => s,
        };
        let sites: Vec<String> = self.views.iter().filter_map(|v| v.site.clone()).collect();
        let name = crate::case::default_name(&sites, time);
        let mut workspace = self.capture_workspace();
        workspace.name = name.clone();
        CaseManifest {
            format: FORMAT,
            name,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created_utc: chrono::Utc::now(),
            time_utc: time,
            span_min,
            sites,
            workspace,
            bookmarks: self.settings.bookmarks.clone(),
            markers: self.settings.markers.clone(),
            zones: self.settings.alert_polygons.clone(),
            strokes: self
                .strokes
                .iter()
                .map(|s| CaseStroke {
                    points: s.points.clone(),
                    rgba: s.color.to_array(),
                })
                .collect(),
            udp_products: self.settings.udp_products.clone(),
            notes: String::new(),
        }
    }

    /// Save the current session as a case file (a save dialog, or a download in a browser).
    pub(crate) fn export_case(&mut self) {
        let case = self.capture_case();
        match crate::dialog::save_bytes(&case.file_name(), "json", case.to_json().as_bytes()) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Case saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("case export failed: {e}");
                self.toast(ToastKind::Error, format!("Case export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Ask for a case file to open; it arrives through the shared import handover.
    pub(crate) fn import_case(&mut self) {
        crate::dialog::request_open(crate::dialog::ImportKind::Case, "");
    }

    /// Open a picked case file: restore its panes, send every one of them to its analysis time
    /// with its replay window, and add its annotations and bookmarks to the ones already here.
    pub(crate) fn open_case(&mut self, import: &crate::dialog::Import, ctx: &egui::Context) {
        let case = match import.text().and_then(|t| CaseManifest::from_json(&t)) {
            Ok(c) => c,
            Err(e) => {
                self.toast(ToastKind::Error, format!("Could not open case: {e}"));
                return;
            }
        };
        self.apply_workspace(&case.workspace, ctx);
        for v in &mut self.views {
            match case.time_utc {
                Some(t) => {
                    v.timeline.date = t.date_naive();
                    v.timeline.following = false;
                    v.timeline.playing = false;
                    v.timeline.seek_target = Some(t);
                    v.timeline.replay_span_min = case.span_min;
                }
                None => v.timeline.go_head(),
            }
        }
        if case.time_utc.is_some() && case.span_min > 0 {
            // As an event replay does: a replay without its warnings and reports is just a loop.
            self.filters.show_alerts = true;
            self.show_storm_reports = true;
        }
        let s = &mut self.settings;
        let added = merge_by(&mut s.bookmarks, &case.bookmarks, |b| b.name.clone())
            + merge_by(&mut s.markers, &case.markers, |m| {
                (m.name.clone(), m.lat.to_bits(), m.lon.to_bits())
            })
            + merge_by(&mut s.alert_polygons, &case.zones, |z| z.name.clone())
            + merge_by(&mut s.udp_products, &case.udp_products, |p| p.name.clone());
        let strokes: Vec<Stroke2d> = case
            .strokes
            .iter()
            .map(|s| Stroke2d {
                points: s.points.clone(),
                // `Color32::to_array` wrote it premultiplied; read it back the same way.
                color: egui::Color32::from_rgba_premultiplied(
                    s.rgba[0], s.rgba[1], s.rgba[2], s.rgba[3],
                ),
            })
            .collect();
        let added = added + merge_by(&mut self.strokes, &strokes, |s| s.points.clone());
        self.settings.save();
        self.rebuild_overlays();
        let what = if added > 0 {
            format!(" ({added} annotation(s) and bookmark(s) added)")
        } else {
            String::new()
        };
        self.toast(
            ToastKind::Success,
            format!("Opened case {}{what}", case.name),
        );
        if !case.notes.is_empty() {
            self.banner(format!("Case: {}", case.name), case.notes.clone());
        }
    }
}
