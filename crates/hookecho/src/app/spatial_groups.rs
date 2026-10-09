//! Camera/site/cursor groups independent of model/run and the legacy analysis clock.
use super::*;
use crate::pane_links::{Dimension, Link};
pub(super) fn same_camera(
    a: crate::render::mercator::Camera,
    b: crate::render::mercator::Camera,
) -> bool {
    a.center == b.center && a.zoom == b.zoom && a.pitch == b.pitch && a.bearing == b.bearing
}
/// Adopt only the selected dimension. Unlinking retains the last resolved state.
pub(super) fn join(views: &mut [MapView], idx: usize, dimension: Dimension, link: Link) {
    if !link.valid() {
        return;
    }
    let owner = views
        .iter()
        .enumerate()
        .find(|(i, v)| *i != idx && link.shares(v.spatial_links.get(dimension)))
        .map(|(i, _)| i);
    *views[idx].spatial_links.get_mut(dimension) = link;
    views[idx].spatial_restore_raw = None;
    if let Some(owner) = owner {
        match dimension {
            Dimension::Camera => {
                views[idx].camera = views[owner].camera;
                views[idx].camera_placed = true;
                views[idx].flight = None;
                views[idx].shown_camera = Some(views[idx].camera);
            }
            Dimension::Site => views[idx].site = views[owner].site.clone(),
            Dimension::Cursor => {}
        }
    }
    match dimension {
        Dimension::Camera => views[idx].spatial_camera_snapshot = views[idx].camera,
        Dimension::Site => views[idx].spatial_site_snapshot = views[idx].site.clone(),
        Dimension::Cursor => {}
    }
}
pub(super) fn sync_sites(views: &mut [MapView], active: usize) {
    let mut changed: Vec<_> = views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.site != v.spatial_site_snapshot)
        .map(|(idx, _)| idx)
        .collect();
    changed.sort_by_key(|idx| *idx != active);
    for idx in changed {
        if views[idx].site == views[idx].spatial_site_snapshot {
            continue;
        }
        let link = views[idx].spatial_links.site;
        let site = views[idx].site.clone();
        for (i, v) in views.iter_mut().enumerate() {
            if i == idx || link.shares(v.spatial_links.site) {
                v.site = site.clone();
                v.spatial_site_snapshot = site.clone();
            }
        }
    }
}
pub(super) fn sync_cameras(views: &mut [MapView], active: usize) {
    let mut changed: Vec<_> = views
        .iter()
        .enumerate()
        .filter(|(_, v)| !same_camera(v.camera, v.spatial_camera_snapshot))
        .map(|(idx, _)| idx)
        .collect();
    changed.sort_by_key(|idx| *idx != active);
    for idx in changed {
        if same_camera(views[idx].camera, views[idx].spatial_camera_snapshot) {
            continue;
        }
        let link = views[idx].spatial_links.camera;
        let camera = views[idx].camera;
        for (i, v) in views.iter_mut().enumerate() {
            if i == idx || link.shares(v.spatial_links.camera) {
                v.camera = camera;
                v.spatial_camera_snapshot = camera;
                if i != idx {
                    v.flight = None;
                    v.shown_camera = Some(camera);
                }
            }
        }
    }
}
pub(super) fn cursor_members(views: &[MapView], owner: usize) -> Vec<usize> {
    let Some(source) = views.get(owner) else {
        return Vec::new();
    };
    if !source.spatial_links.cursor.enabled {
        return Vec::new();
    }
    views
        .iter()
        .enumerate()
        .filter(|(_, v)| source.spatial_links.cursor.shares(v.spatial_links.cursor))
        .map(|(i, _)| i)
        .collect()
}
pub(super) fn all_linked(views: &[MapView]) -> bool {
    views.first().is_some_and(|source| {
        Dimension::ALL.into_iter().all(|dimension| {
            views.iter().all(|view| {
                source
                    .spatial_links
                    .get(dimension)
                    .shares(view.spatial_links.get(dimension))
            })
        })
    })
}
pub(super) fn picker(ui: &mut egui::Ui, pane: usize, links: &mut crate::pane_links::SpatialLinks) {
    use crate::ui::a11y::Named as _;
    let touch = crate::ui::workstation::touch(ui.ctx()) || crate::platform::phone_layout();
    if touch {
        ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(44.0);
    }
    ui.weak(format!("Pane {} · Spatial links", pane + 1));
    for dimension in Dimension::ALL {
        let link = links.get_mut(dimension);
        ui.horizontal_wrapped(|ui| {
            ui.label(if touch && dimension == Dimension::Cursor { "Geo cursor" } else { dimension.label() });
            let mut group = link.enabled.then_some(link.group);
            let _response = egui::ComboBox::from_id_salt(("spatial_group", pane, dimension.label())).width(104.0)
                .selected_text(group.map_or_else(|| "Independent".into(), |g| format!("Group {g}")))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut group, None, "Independent");
                    for g in 1..=crate::view::MAX_PANES as u8 { ui.selectable_value(&mut group, Some(g), format!("Group {g}")); }
                }).response.named(&format!("Pane {} {} link group", pane + 1, dimension.label()))
                .on_hover_text("Only members of the same enabled group share this dimension. Independent retains the last resolved state.");
            #[cfg(test)]
            assert!(!touch || _response.rect.height() >= 44.0, "spatial group picker needs a touch target");
            link.enabled = group.is_some();
            if let Some(group) = group { link.group = group; }
        });
    }
    ui.add(egui::Label::new(egui::RichText::new("Joining adopts that group's camera or radar site. Independent keeps the current view. Model/run groups are separate. Analysis time links within its own groups (below) while Link times is on.").weak()).wrap());
}

/// Pane `pane`'s analysis-time group (M5.1, 1008.md E1): panes in the same group share one
/// linked clock while Link times is on, so a live group and an archive group can run side by side.
pub(super) fn time_group_picker(ui: &mut egui::Ui, pane: usize, group: &mut u8, linking: bool) {
    use crate::ui::a11y::Named as _;
    ui.horizontal_wrapped(|ui| {
        ui.label("Analysis time");
        egui::ComboBox::from_id_salt(("time_group", pane))
            .width(104.0)
            .selected_text(format!("Group {group}"))
            .show_ui(ui, |ui| {
                for g in 1..=crate::view::MAX_PANES as u8 {
                    ui.selectable_value(group, g, format!("Group {g}"));
                }
            })
            .response
            .named(&format!("Pane {} analysis time group", pane + 1))
            .on_hover_text(
                "Panes in the same group share one analysis time: scrubbing or a jump moves the \
                 whole group and no other. A pane alone in its group keeps its own time; joining \
                 adopts the group's.",
            );
    });
    if !linking {
        ui.weak("Link times is off, so every pane keeps its own time.");
    }
}

impl HookEchoApp {
    pub(super) fn all_pane_links_on(&self) -> bool {
        self.link_times
            && self.link_storm
            && all_linked(&self.views)
            && self
                .views
                .iter()
                .all(|v| v.time_group == self.views[0].time_group)
    }
    pub(crate) fn spatial_group_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Pane links")
            .id_salt("spatial_links_controls")
            .show(ui, |ui| {
                if self.views[self.active].spatial_restore_raw.is_some() {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Saved spatial links unavailable. Choose links to replace them.",
                    );
                }
                let previous = self.views[self.active].spatial_links;
                let mut next = previous;
                picker(ui, self.active, &mut next);
                for dimension in Dimension::ALL {
                    if previous.get(dimension) != next.get(dimension) {
                        join(&mut self.views, self.active, dimension, next.get(dimension));
                        self.linked_probe = None;
                    }
                }
                ui.separator();
                let mut group = self.views[self.active].time_group;
                time_group_picker(ui, self.active, &mut group, self.link_times);
                // Joining adopts the group's analysis time on the next sync
                // (`pane_time::sync_time_groups`); the group left keeps its own.
                self.views[self.active].time_group = group;
            });
    }
    pub(super) fn toggle_spatial_link(&mut self, toggle: OverlayToggle) {
        let dimension = match toggle {
            OverlayToggle::LinkCameras => Dimension::Camera,
            OverlayToggle::LinkSite => Dimension::Site,
            OverlayToggle::LinkCursor => Dimension::Cursor,
            _ => return,
        };
        let mut link = self.views[self.active].spatial_links.get(dimension);
        link.enabled = !link.enabled;
        join(&mut self.views, self.active, dimension, link);
        self.linked_probe = None;
    }
    pub(super) fn link_all_cameras(&mut self) {
        let camera = self.views[self.active].camera;
        for view in &mut self.views {
            view.spatial_restore_raw = None;
            view.spatial_links.camera = Link {
                group: 1,
                enabled: true,
            };
            view.camera = camera;
            view.spatial_camera_snapshot = camera;
            view.flight = None;
            view.shown_camera = Some(camera);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The time-group row under the spatial links, for review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the time group picker"]
    fn gpu_time_group_picker_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the picker");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m5.1");
        std::fs::create_dir_all(&destination).unwrap();
        gpu.save(&destination.join("time-group-picker.png"), 420, 250, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(400.0);
                let mut links = crate::pane_links::SpatialLinks::legacy(true, true, true);
                picker(ui, 1, &mut links);
                ui.separator();
                let mut group = 2;
                time_group_picker(ui, 1, &mut group, true);
            });
        })
        .unwrap();
    }
    use crate::pane_links::SpatialLinks;
    use crate::render::mercator::Camera;
    fn pane(site: &str, group: u8) -> MapView {
        let mut view = MapView::new(Some(site.into()), Camera::at_lonlat(-97.0, 35.0, 8.0));
        view.spatial_links = SpatialLinks::legacy(true, true, true);
        for dimension in Dimension::ALL {
            view.spatial_links.get_mut(dimension).group = group;
        }
        view
    }
    #[test]
    fn spatial_groups_link_all_status_requires_membership_across_every_pane() {
        let mut views = vec![pane("KTLX", 1), pane("KDMX", 2)];
        assert!(!all_linked(&views));
        for dimension in Dimension::ALL {
            views[1].spatial_links.get_mut(dimension).group = 1;
        }
        assert!(all_linked(&views));
        views[1].spatial_links.cursor.enabled = false;
        assert!(!all_linked(&views));
    }
    #[test]
    fn spatial_groups_two_cameras_move_independently_and_focus_does_not_retarget() {
        let mut views = vec![
            pane("KTLX", 1),
            pane("KDMX", 2),
            pane("KTLX", 1),
            pane("KDMX", 2),
        ];
        views[0].camera = Camera::at_lonlat(-100.0, 40.0, 10.0);
        views[0].camera.pitch = 35.0;
        views[0].camera.bearing = -72.0;
        views[1].camera = Camera::at_lonlat(-85.0, 28.0, 6.0);
        sync_cameras(&mut views, 0);
        assert!(same_camera(views[0].camera, views[2].camera));
        assert!(same_camera(views[1].camera, views[3].camera));
        assert!(!same_camera(views[0].camera, views[1].camera));
        let camera = views[0].camera;
        sync_cameras(&mut views, 1);
        assert!(same_camera(views[0].camera, camera));
    }
    #[test]
    fn spatial_groups_site_changes_keep_products_tilts_and_analysis_clocks() {
        let mut views = vec![pane("KTLX", 1), pane("KDMX", 2), pane("KTLX", 1)];
        views[2].moment = Moment::Velocity;
        views[2].tilt = 3;
        views[2].timeline.seek_target = DateTime::from_timestamp(1_700_000_000, 0);
        let target = views[2].timeline.seek_target;
        views[0].site = Some("KOUN".into());
        sync_sites(&mut views, 1);
        assert_eq!(views[2].site.as_deref(), Some("KOUN"));
        assert_eq!(views[1].site.as_deref(), Some("KDMX"));
        assert_eq!(views[2].moment, Moment::Velocity);
        assert_eq!(views[2].tilt, 3);
        assert_eq!(views[2].timeline.seek_target, target);
        views[0].site = None;
        sync_sites(&mut views, 0);
        assert!(views[2].site.is_none());
    }
    #[test]
    fn spatial_groups_join_unlink_and_dimensions_keep_the_last_resolved_state() {
        let mut views = vec![pane("KTLX", 1), pane("KDMX", 2)];
        views[0].camera = Camera::at_lonlat(-100.0, 42.0, 9.0);
        let camera_link = Link {
            group: 1,
            enabled: true,
        };
        join(&mut views, 1, Dimension::Camera, camera_link);
        assert!(same_camera(views[0].camera, views[1].camera));
        assert_eq!(views[1].site.as_deref(), Some("KDMX"));
        assert_eq!(views[1].spatial_links.site.group, 2);
        assert_eq!(views[1].spatial_links.cursor.group, 2);
        join(
            &mut views,
            1,
            Dimension::Camera,
            Link {
                enabled: false,
                ..camera_link
            },
        );
        let retained = views[1].camera;
        views[0].camera.zoom += 1.0;
        sync_cameras(&mut views, 0);
        assert!(same_camera(views[1].camera, retained));
        join(&mut views, 1, Dimension::Site, camera_link);
        assert_eq!(views[1].site.as_deref(), Some("KTLX"));
        join(
            &mut views,
            1,
            Dimension::Site,
            Link {
                enabled: false,
                ..camera_link
            },
        );
        views[0].site = Some("KOUN".into());
        sync_sites(&mut views, 0);
        assert_eq!(views[1].site.as_deref(), Some("KTLX"));
    }
    #[test]
    fn spatial_groups_cursor_members_follow_hover_owner_after_reorder_and_removal() {
        let mut views = vec![pane("KTLX", 1), pane("KDMX", 2), pane("KTLX", 1)];
        assert_eq!(cursor_members(&views, 0), vec![0, 2]);
        views.swap(0, 1);
        assert_eq!(cursor_members(&views, 1), vec![1, 2]);
        views.remove(1);
        assert_eq!(cursor_members(&views, 1), vec![1]);
        join(
            &mut views,
            0,
            Dimension::Cursor,
            Link {
                group: 1,
                enabled: true,
            },
        );
        assert_eq!(cursor_members(&views, 1), vec![0, 1]);
        views[1].spatial_links.cursor.enabled = false;
        assert!(cursor_members(&views, 1).is_empty());
        assert!(cursor_members(&views, 9).is_empty());
    }
    #[test]
    fn spatial_groups_joining_cursor_does_not_swallow_pending_camera_or_site_edits() {
        let mut views = vec![pane("KTLX", 1), pane("KDMX", 1)];
        views[0].camera.zoom = 12.0;
        views[0].site = Some("KOUN".into());
        join(
            &mut views,
            0,
            Dimension::Cursor,
            Link {
                group: 2,
                enabled: true,
            },
        );
        sync_sites(&mut views, 0);
        sync_cameras(&mut views, 0);
        assert_eq!(views[1].site.as_deref(), Some("KOUN"));
        assert_eq!(views[1].camera.zoom, 12.0);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "explicit production UI capture requires a GPU"]
    fn gpu_spatial_groups_controls() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU for spatial link controls");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/spatial-groups/ui");
        std::fs::create_dir_all(&destination).unwrap();
        for (name, links) in [
            ("independent", SpatialLinks::default()),
            (
                "mixed",
                SpatialLinks {
                    camera: Link {
                        group: 2,
                        enabled: true,
                    },
                    site: Link {
                        group: 1,
                        enabled: false,
                    },
                    cursor: Link {
                        group: 3,
                        enabled: true,
                    },
                },
            ),
        ] {
            for (width, phone) in [(240, true), (320, true), (640, false)] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    420,
                    |ui| {
                        crate::ui::workstation::set_touch(ui.ctx(), phone);
                        egui::Frame::NONE.inner_margin(12).show(ui, |ui| {
                            let mut links = links;
                            picker(ui, 2, &mut links);
                            assert!(ui.min_rect().right() <= ui.max_rect().right() + 0.5);
                            assert!(ui.min_rect().bottom() <= 420.0);
                        });
                    },
                )
                .unwrap();
            }
        }
        let time = DateTime::parse_from_rfc3339("2026-10-05T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let rows = vec![
            crate::ui::cursor_probe::ProbeRow {
                pane: 0,
                source: "HRRR / Composite reflectivity; run 2026-10-05 12:00 UTC".into(),
                product: "Composite reflectivity".into(),
                time: Some(time),
                value: Some("54 dBZ".into()),
                folded: false,
            },
            crate::ui::cursor_probe::ProbeRow {
                pane: 2,
                source: "KTLX".into(),
                product: "Velocity".into(),
                time: Some(time + chrono::Duration::minutes(3)),
                value: None,
                folded: true,
            },
        ];
        for (width, narrow) in [(240, true), (320, true), (1000, false)] {
            gpu.save(
                &destination.join(format!("probe-{width}.png")),
                width,
                300,
                |ui| {
                    egui::Frame::NONE.inner_margin(12).show(ui, |ui| {
                        crate::ui::cursor_probe::body(ui, &rows, None, narrow);
                        assert!(ui.min_rect().right() <= ui.max_rect().right() + 0.5);
                        assert!(ui.min_rect().bottom() <= 300.0);
                    });
                },
            )
            .unwrap();
        }
    }
}
