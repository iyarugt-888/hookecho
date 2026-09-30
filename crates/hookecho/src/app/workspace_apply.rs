//! Workspaces applied and captured: the panes, links, layers and chrome a saved arrangement
//! restores, and what the app skips from it and says so. Moved out of `app.rs` unchanged
//! (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Snapshot the current arrangement. Auto-named: naming things is a peacetime activity.
    pub(crate) fn capture_workspace(&mut self) -> crate::workspace::Workspace {
        let overlays_on: Vec<String> = OverlayToggle::ALL
            .into_iter()
            .filter(|t| !t.session_only() && *self.overlay_flag(*t))
            .map(|t| t.slug())
            .collect();
        crate::workspace::Workspace {
            name: format!("Workspace {}", self.settings.workspaces.len() + 1),
            pane_layout: self.pane_layout,
            panes: self
                .views
                .iter()
                .map(crate::workspace::PaneSnap::capture)
                .collect(),
            active: self.active,
            link_cameras: self.link_cameras,
            link_times: self.link_times,
            lock_source_time: self.lock_source_time,
            link_site: self.link_site,
            link_cursor: self.link_cursor,
            link_storm: self.link_storm,
            overlays_on,
            // A workspace you saved records the sites you had open; only the shipped starters
            // adopt whatever is on screen.
            adopt_site: false,
            // The workspace-wide list stays as the union across panes: it is what an older build
            // reads, and what a pane snapshot written before per-pane layers falls back to.
            fields_on: crate::render::FieldLayer::DRAW_ORDER
                .iter()
                .filter(|l| self.field_wanted(**l))
                .map(|l| l.slug().to_string())
                .collect(),
            chrome: Some(self.capture_chrome()),
            // A sounding open when the snapshot is taken is part of how this analyst works here.
            sound_center: self.sounding_window.open,
            extra: Default::default(),
        }
    }

    /// Restore a saved arrangement. Panes come back empty of data and fill through the normal
    /// poll, exactly as a freshly split pane does.
    pub(crate) fn apply_workspace(
        &mut self,
        ws: &crate::workspace::Workspace,
        ctx: &egui::Context,
    ) {
        if ws.panes.is_empty() {
            return;
        }
        // What this build skips from the file is said, not silently dropped (ROADMAP_2 §12.2).
        let problems = crate::workspace::problems(ws);
        if !problems.is_empty() {
            let msg = format!(
                "Workspace \u{201c}{}\u{201d}: {}",
                ws.name,
                problems.join("; ")
            );
            log::warn!("{msg}");
            self.error_chip = Some((msg, ctx.input(|i| i.time)));
        }
        // Where the analyst was looking, before the panes move: what `sound_center` sounds.
        let looking_at = {
            let c = self.views[self.active].camera.center;
            crate::render::mercator::world_to_lonlat(c.0, c.1)
        };
        let adopted = ws
            .adopt_site
            .then(|| self.views[self.active].site.clone())
            .flatten();
        self.set_pane_count(ws.panes.len());
        self.pane_layout = ws.pane_layout;
        for (v, snap) in self.views.iter_mut().zip(&ws.panes) {
            snap.apply(v);
            if v.site.is_none() {
                if let Some(site) = &adopted {
                    v.site = Some(site.clone());
                    // The starter's camera is a placeholder over the plains; let the site
                    // recenter this pane the way a fresh one does.
                    v.camera_placed = false;
                }
            }
        }
        self.active = ws.active.min(self.views.len() - 1);
        self.link_cameras = ws.link_cameras;
        self.link_times = ws.link_times;
        self.lock_source_time = ws.lock_source_time;
        self.link_site = ws.link_site;
        self.link_cursor = ws.link_cursor;
        self.link_storm = ws.link_storm;
        self.linked_probe = None;
        self.linked_analysis = pane_time::LinkedTimeState::default();
        // Overlay names this build doesn't know are skipped, same as the settings restore.
        for t in OverlayToggle::ALL {
            if t.session_only() {
                continue;
            }
            *self.overlay_flag(t) = ws.overlays_on.iter().any(|s| *s == t.slug());
        }
        // Same rule for the national field layers: an unknown slug is a layer this build
        // doesn't have, which is a thing to skip rather than an error.
        for (v, snap) in self.views.iter_mut().zip(&ws.panes) {
            // A pane snapshot written before per-pane layers existed carries `None`, and falls
            // back to the workspace-wide list so an old file still restores what it meant. An
            // empty list is a pane that had its layers off, which is a decision, not a gap.
            let list = snap.fields_on.as_ref().unwrap_or(&ws.fields_on);
            v.fields_on = crate::render::FieldLayer::DRAW_ORDER
                .iter()
                .copied()
                .filter(|l| list.iter().any(|s| s == l.slug()))
                .collect();
        }
        if let Some(c) = &ws.chrome {
            self.apply_chrome(c, ctx);
        }
        if ws.sound_center {
            self.fetch_sounding(looking_at.0, looking_at.1);
        }
        self.rebuild_overlays();
        self.pane_shown.clear();
    }

    pub(crate) fn set_pane_count(&mut self, n: usize) {
        let n = n.clamp(1, crate::view::MAX_PANES);
        while self.views.len() < n {
            let src = &self.views[self.active];
            let (site, camera, basemap, tilt, date) = (
                src.site.clone(),
                src.camera,
                src.basemap,
                src.tilt,
                src.timeline.date,
            );
            // Split a scrubbed view and the new panes have to land on the same instant, not on
            // live. Without this, splitting an archive view gave one populated pane and three
            // empty ones, each quietly polling today's head for a site that has no storm on it.
            let seek = (!src.timeline.following)
                .then(|| src.timeline.current().and_then(|id| id.date_time()))
                .flatten();
            let mut v = MapView::new(site, camera);
            v.smooth = self.settings.smooth_radar;
            v.basemap = basemap;
            v.tilt = tilt;
            v.timeline.date = date;
            v.timeline.following = seek.is_none();
            v.timeline.seek_target = seek;
            v.moment = Moment::ALL[self.views.len() % Moment::ALL.len()];
            self.views.push(v);
        }
        self.views.truncate(n);
        if self.active >= n {
            self.active = n - 1;
        }
        self.pane_shown.clear();
    }
}
