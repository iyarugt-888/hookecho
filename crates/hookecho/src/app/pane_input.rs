//! A pane's input: the divider and map interaction, pan, wheel and pinch zoom, two-finger
//! gestures, long press, and clicks on the map. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7); it returns the pane's response for the drawing that follows, or `None`
//! where a click ended the pane's frame early.

use super::*;

impl HookEchoApp {
    pub(crate) fn pane_input(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        idx: usize,
        prect: egui::Rect,
        vp: (f32, f32),
    ) -> Option<egui::Response> {
        // The divider and map are one interaction target. Two overlapping `ui.interact` calls
        // made swipe unreliable because egui could give the press to the full-pane map before the
        // narrower divider saw it. Decide ownership from the press origin once, then preserve it
        // for the whole drag so the moving handle never drops the gesture.
        let response = ui
            .interact(
                prect,
                egui::Id::new(("pane", idx)),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(if self.views[idx].swipe_compare {
                let x = prect.left() + prect.width() * self.views[idx].swipe_fraction;
                if ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|pos| (pos.x - x).abs() <= 12.0)
                {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::Default
                }
            } else {
                egui::CursorIcon::Default
            });
        if self.views[idx].swipe_compare {
            let x = prect.left() + prect.width() * self.views[idx].swipe_fraction;
            let hit = egui::Rect::from_min_max(
                egui::pos2(x - 12.0, prect.top()),
                egui::pos2(x + 12.0, prect.bottom()),
            );
            if response.drag_started() {
                self.views[idx].swipe_dragging = ui.input(|i| {
                    i.pointer
                        .press_origin()
                        .is_some_and(|pos| hit.contains(pos))
                });
            }
            if self.views[idx].swipe_dragging && response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    self.views[idx].swipe_fraction =
                        Self::swipe_fraction_from_pointer(pos.x, prect.left(), prect.width());
                    self.active = idx;
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
            if response.drag_stopped() {
                self.views[idx].swipe_dragging = false;
            }
        } else {
            self.views[idx].swipe_dragging = false;
        }
        let swipe_dragging = self.views[idx].swipe_dragging;

        // --- Input (mutates this pane's camera / selects it active) ---
        // During a multi-touch gesture the first finger still drives the egui pointer, so a pinch
        // would ALSO register as a drag and fight the zoom — the gesture block below owns both
        // pan and zoom while two fingers are down.
        let gesture = ui.input(|i| i.multi_touch());
        if gesture.is_some() {
            self.last_gesture_end = Some(wxdata::clock::Instant::now());
        }
        // A finger still down after the other lifted is the tail of a pinch, not a new drag or a
        // tap on the map. 150 ms is long enough to cover a normal two-finger lift and short
        // enough to be invisible when you really did mean to tap.
        let gesture_tail = self
            .last_gesture_end
            .is_some_and(|t| t.elapsed().as_millis() < 150);
        let quiet = gesture.is_none() && !gesture_tail;
        // Watch-zone tool: a double-click (or Enter) closes the ring being clicked out.
        if self.tool == MapTool::AlertZone
            && self.zone_pts.len() >= 3
            && (response.double_clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            let mut ring = std::mem::take(&mut self.zone_pts);
            // The two clicks of the closing double-click each dropped a vertex on the same spot.
            ring.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
            let n = self.settings.alert_polygons.len() + 1;
            self.zone_naming = Some((ring, format!("Zone {n}")));
            self.tool = MapTool::Interrogate;
        }
        // The draw tool takes the drag away from the pan, the same deal the measure tool makes
        // with the click: while it's armed, a drag draws. Disarm it (Esc / another tool) to pan.
        let tracking = self.storm_track_input(idx, prect, &response, ui, quiet && !swipe_dragging);
        if tracking {
            // The storm-motion tool took the drag (or holds a handle): no pan under it.
        } else if self.tool == MapTool::Draw && quiet && !swipe_dragging {
            if response.dragged() {
                self.active = idx;
                if let Some(pos) = response.interact_pointer_pos() {
                    let cam = self.views[idx].camera;
                    let px = (pos.x - prect.left(), pos.y - prect.top());
                    let w = cam.screen_to_world(px, vp);
                    let ll = crate::render::mercator::world_to_lonlat(w.0, w.1);
                    draw_append(
                        &mut self.strokes,
                        [ll.0, ll.1],
                        self.draw_color,
                        response.drag_started(),
                    );
                }
            }
        } else if response.dragged() && quiet && !swipe_dragging {
            self.active = idx;
            let d = response.drag_delta();
            if self.views[idx].map_3d.enabled && response.dragged_by(egui::PointerButton::Secondary)
            {
                self.views[idx].camera.bearing =
                    (self.views[idx].camera.bearing - d.x * 0.35 + 180.0).rem_euclid(360.0) - 180.0;
                self.views[idx].camera.pitch = (self.views[idx].camera.pitch + d.y * 0.25)
                    .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
            } else {
                match self.tap_zoom {
                    // Double-tap-drag: the map zoom every phone map has, and the only one you can do
                    // one-handed. Drag up to zoom in, anchored on the point that was tapped, so the
                    // thing you double-tapped is the thing that stays put.
                    Some(anchor) => {
                        let cursor = (anchor.x - prect.left(), anchor.y - prect.top());
                        self.views[idx]
                            .camera
                            .zoom_at(-d.y as f64 * 0.01, cursor, vp);
                    }
                    None => {
                        self.views[idx].camera.pan_pixels(d.x, d.y, vp);
                        self.follow_cell = None; // a manual pan takes over the camera
                    }
                }
            }
        }
        if cfg!(target_os = "android") {
            if response.drag_started() {
                // Within a third of a second of the last tap and within a thumb's width of it:
                // this is the second tap, still down. Anything else is an ordinary drag.
                self.tap_zoom = response.interact_pointer_pos().filter(|p| {
                    self.last_tap.is_some_and(|(t, at)| {
                        ui.input(|i| i.time) - t < 0.35 && at.distance(*p) < 44.0
                    })
                });
            }
            if response.drag_stopped() {
                self.tap_zoom = None;
                // One zoom per double tap: the next one has to be armed by a fresh tap.
                self.last_tap = None;
            }
        }
        // Wheel, trackpad, and pinch all land here. `zoom_delta` is a scale factor carrying the
        // macOS/precision-touchpad pinch gesture (and ctrl+wheel, which egui folds into the same
        // signal and subtracts from the scroll delta, so the two never double-apply). Horizontal
        // scroll pans: on a trackpad a two-finger swipe is the obvious way to move the map, and on
        // a mouse it is a tilt wheel nobody was using.
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if let Some(pos) = response.hover_pos().filter(|p| prect.contains(*p)) {
            let cursor = (pos.x - prect.left(), pos.y - prect.top());
            // ROADMAP_NEW J3: share this pane's hovered geo point with every other pane. Reused
            // directly as the probe table's sample point in `paint_linked_cursor`, called once
            // after every pane has had a chance to update it this frame.
            if self.views[idx].spatial_links.cursor.enabled {
                let w = self.views[idx].camera.screen_to_world(cursor, vp);
                self.linked_probe = Some((idx, crate::render::mercator::world_to_lonlat(w.0, w.1)));
            }
            // `zoom_delta()` reports a live touchscreen pinch too, which the gesture block below
            // already owns (and it is the one that knows about the mobile chrome) — skip it here
            // or a phone pinch zooms twice.
            if gesture.is_none() && (zoom - 1.0).abs() > f32::EPSILON {
                self.active = idx;
                self.views[idx]
                    .camera
                    .zoom_at((zoom as f64).log2(), cursor, vp);
            }
            if scroll.y.abs() > 0.0 {
                self.active = idx;
                self.views[idx]
                    .camera
                    .zoom_at(scroll.y as f64 * 0.005, cursor, vp);
            }
            if scroll.x.abs() > 0.0 {
                self.active = idx;
                self.views[idx].camera.pan_pixels(scroll.x, 0.0, vp);
                self.follow_cell = None; // a manual pan takes over the camera
            }
        }
        // Two-finger gesture (touchscreens): pan by the gesture's translation, and zoom by the
        // pinch. `zoom_delta` is a scale factor, so its log2 is the change in the camera's log2
        // zoom level; anchor it at the gesture center so the pinched point stays put. Fires for
        // the pane the gesture centers over. No-op with no touch.
        if let Some(mt) = gesture {
            // The mobile chrome floats over the map, and `multi_touch()` is raw input with no
            // notion of which layer the fingers are on — so a pinch on the bottom sheet used to
            // zoom the map underneath it. The chrome publishes what it covers; skip those rects.
            // Explicit rects cover the always-on chrome; the layer test covers every window and
            // popup on top of it, which is what keeps a pinch on a Skew-T or a full-screen
            // settings surface from also zooming the map behind it.
            // `is_pointer_over_egui()` cannot be used here: it tests egui's single pointer
            // position, which during a two-finger gesture is one arbitrary finger, and it treats
            // the map's own background layer as "over egui" once the central panel has consumed
            // the root ui's available rect — so it answered true over bare map and killed every
            // pinch. Ask about the gesture's own center instead: any layer above the background
            // there is real chrome.
            let over_layer = ui
                .ctx()
                .layer_id_at(mt.center_pos)
                .is_some_and(|l| l.order != egui::Order::Background);
            let occluded = over_layer
                || self
                    .mobile_occlusion
                    .iter()
                    .any(|r| r.contains(mt.center_pos));
            if prect.contains(mt.center_pos) && !occluded {
                self.active = idx;
                // Zoom first, then pan: the translation is in screen pixels, and applying it at
                // the pre-zoom scale over-moves the map by the pinch's own scale factor — which is
                // what made the anchor trail the fingers.
                //
                // Belt-and-suspenders against a runaway single-frame gesture (reported on web
                // touchscreens: the map "flies away" with multiple fingers down). egui's own
                // `TouchState` already nulls the previous sample when the touch count changes
                // within a frame (`begin_pass`'s `added_or_removed_touches` guard), so a clean
                // 2-vs-3-finger centroid jump is not the mechanism — but browser touch-event
                // dispatch upstream of egui is out of this app's control, so cap what one frame's
                // gesture is allowed to do regardless of where an anomalous sample comes from. A
                // real pinch/pan between consecutive touch samples never moves the centroid by a
                // large fraction of the pane in one frame or halves/doubles the zoom in one frame.
                let max_translation = prect.size().min_elem() * 0.4;
                if mt.translation_delta.length() > max_translation {
                    log::warn!(
                    "multi-touch gesture translation clamped: {:.0}px requested, {:.0}px pane limit",
                    mt.translation_delta.length(),
                    max_translation
                );
                }
                let zoom_log2 = (mt.zoom_delta as f64).log2().clamp(-1.0, 1.0);
                if (mt.zoom_delta - 1.0).abs() > f32::EPSILON {
                    let cursor = (
                        mt.center_pos.x - prect.left(),
                        mt.center_pos.y - prect.top(),
                    );
                    self.views[idx].camera.zoom_at(zoom_log2, cursor, vp);
                }
                if self.views[idx].map_3d.enabled && mt.rotation_delta.abs() > 0.001 {
                    self.views[idx].camera.bearing =
                        (self.views[idx].camera.bearing - mt.rotation_delta.to_degrees() + 180.0)
                            .rem_euclid(360.0)
                            - 180.0;
                }
                let t = mt.translation_delta.clamp(
                    egui::vec2(-max_translation, -max_translation),
                    egui::vec2(max_translation, max_translation),
                );
                // On a 3D map the two fingers' vertical slide tilts the view instead of panning it
                // (one finger still pans): slide up to lean the map back toward the horizon, down
                // to stand it up flat.
                let (pan, pitch_delta) = split_two_finger_slide(t, self.views[idx].map_3d.enabled);
                if pitch_delta != 0.0 {
                    let cam = &mut self.views[idx].camera;
                    cam.pitch = (cam.pitch + pitch_delta)
                        .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
                }
                if pan != egui::Vec2::ZERO {
                    self.views[idx].camera.pan_pixels(pan.x, pan.y, vp);
                    self.follow_cell = None; // a manual pan takes over the camera (pinch-zoom does not)
                }
            }
        }
        // Long-press explores features, whatever tool is armed. A phone has no right-click and no
        // hover, so the press that means "tell me about this" is the press people already try.
        let long_press = cfg!(target_os = "android") && response.long_touched();
        if long_press {
            // Before anything is drawn: the buzz is what says the press was heard.
            crate::platform::haptic(crate::platform::Haptic::Press);
        }
        // A click on a tornado marker opens or closes its web (drawn below); it is not also a
        // click on the storm cell or map under it. Where the markers sat is last frame's.
        let on_circulation = response.interact_pointer_pos().is_some_and(|p| {
            ui.ctx()
                .data(|d| d.get_temp::<Vec<egui::Rect>>(egui::Id::new(("circulation_hits", idx))))
                .unwrap_or_default()
                .iter()
                .any(|r| r.contains(p))
        });
        // A click or long press on the map (`app::map_click`). It returns true where the code
        // used to leave `render_pane` early (a gauge or station card opened), so this does too.
        if (response.clicked() || long_press)
            && quiet
            && !on_circulation
            && self.handle_map_click(ui, ctx, idx, prect, vp, &response, long_press)
        {
            return None;
        }
        Some(response)
    }
}
