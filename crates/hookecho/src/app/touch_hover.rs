//! Tooltips on a touch screen. Every `on_hover_*` tooltip in the app (several hundred of them, and
//! the map's own hover readings) needs a pointer that rests on something, which a finger only does
//! while it is down: egui shows the tooltip during a press-and-hold and drops it the moment the
//! finger lifts, so it can be seen but not read.
//!
//! Here a press held still for [`HOLD_SECS`] pins it instead: when that finger lifts, the
//! `PointerGone` that follows is dropped, so the hover stays where the finger was and its tooltip
//! stays up until the next touch anywhere. A press-and-hold is egui's secondary click (a context
//! menu), never a primary one, so holding a button to read it does not press it. A tap, a drag or
//! a pinch behaves exactly as before.

/// How long a still press must be held to pin its tooltip: egui's own long-press time
/// (`InputOptions::max_click_duration`), past which a release is no longer a click.
pub(crate) const HOLD_SECS: f64 = 0.8;
/// How far a finger may wander and still be held "still", in points: egui's click distance.
const STILL_PTS: f32 = 6.0;

/// The touch being followed, between frames.
#[derive(Clone, Debug, Default)]
pub(crate) struct TouchHold {
    /// The one finger down: where and when it started. `None` with none down, or more than one.
    start: Option<(f64, egui::Pos2)>,
    /// It moved too far, or a second finger joined: not a press-and-hold.
    spoiled: bool,
    /// The last press-and-hold ended: its hover is pinned until the next touch.
    pinned: bool,
}

/// Follow this frame's touches and, after a press-and-hold, drop the `PointerGone` that would end
/// its hover. `now` is the input's time, in seconds.
pub(crate) fn pin_long_press_hover(state: &mut TouchHold, raw: &mut egui::RawInput, now: f64) {
    let mut fingers_down = usize::from(state.start.is_some());
    raw.events.retain(|e| match e {
        egui::Event::Touch { phase, pos, .. } => {
            match phase {
                egui::TouchPhase::Start => {
                    state.pinned = false;
                    fingers_down += 1;
                    if fingers_down == 1 {
                        state.start = Some((now, *pos));
                        state.spoiled = false;
                    } else {
                        state.spoiled = true;
                    }
                }
                egui::TouchPhase::Move => {
                    if state
                        .start
                        .is_some_and(|(_, at)| at.distance(*pos) > STILL_PTS)
                    {
                        state.spoiled = true;
                    }
                }
                egui::TouchPhase::End => {
                    if let Some((at, _)) = state.start.take() {
                        state.pinned = !state.spoiled && now - at >= HOLD_SECS;
                    }
                    fingers_down = fingers_down.saturating_sub(1);
                }
                egui::TouchPhase::Cancel => {
                    state.start = None;
                    state.pinned = false;
                    fingers_down = fingers_down.saturating_sub(1);
                }
            }
            true
        }
        egui::Event::PointerGone => !state.pinned,
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(phase: egui::TouchPhase, x: f32) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(1),
            phase,
            pos: egui::pos2(x, 100.0),
            force: None,
        }
    }

    fn frame(state: &mut TouchHold, now: f64, events: Vec<egui::Event>) -> Vec<egui::Event> {
        let mut raw = egui::RawInput {
            events,
            ..Default::default()
        };
        pin_long_press_hover(state, &mut raw, now);
        raw.events
    }

    fn gone(events: &[egui::Event]) -> bool {
        events.iter().any(|e| matches!(e, egui::Event::PointerGone))
    }

    #[test]
    fn a_held_press_keeps_its_hover_until_the_next_touch() {
        let mut s = TouchHold::default();
        frame(&mut s, 0.0, vec![touch(egui::TouchPhase::Start, 50.0)]);
        frame(&mut s, 0.5, vec![touch(egui::TouchPhase::Move, 52.0)]);
        let lifted = frame(
            &mut s,
            1.0,
            vec![touch(egui::TouchPhase::End, 52.0), egui::Event::PointerGone],
        );
        assert!(!gone(&lifted), "the hover stays: its tooltip can be read");
        // Nothing else happens: still pinned.
        assert!(!gone(&frame(&mut s, 2.0, vec![egui::Event::PointerGone])));
        // The next touch, anywhere, ends it as usual.
        frame(&mut s, 3.0, vec![touch(egui::TouchPhase::Start, 300.0)]);
        let tapped = frame(
            &mut s,
            3.1,
            vec![
                touch(egui::TouchPhase::End, 300.0),
                egui::Event::PointerGone,
            ],
        );
        assert!(gone(&tapped), "a tap lets go of the pointer as before");
    }

    #[test]
    fn taps_drags_and_pinches_are_untouched() {
        // A short tap.
        let mut s = TouchHold::default();
        frame(&mut s, 0.0, vec![touch(egui::TouchPhase::Start, 50.0)]);
        let e = frame(
            &mut s,
            0.2,
            vec![touch(egui::TouchPhase::End, 50.0), egui::Event::PointerGone],
        );
        assert!(gone(&e));
        // A long drag (panning the map).
        let mut s = TouchHold::default();
        frame(&mut s, 0.0, vec![touch(egui::TouchPhase::Start, 50.0)]);
        frame(&mut s, 0.6, vec![touch(egui::TouchPhase::Move, 120.0)]);
        let e = frame(
            &mut s,
            1.5,
            vec![
                touch(egui::TouchPhase::End, 120.0),
                egui::Event::PointerGone,
            ],
        );
        assert!(gone(&e));
        // A held pinch: a second finger spoils it.
        let mut s = TouchHold::default();
        let second = egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(2),
            phase: egui::TouchPhase::Start,
            pos: egui::pos2(200.0, 100.0),
            force: None,
        };
        frame(
            &mut s,
            0.0,
            vec![touch(egui::TouchPhase::Start, 50.0), second],
        );
        let e = frame(
            &mut s,
            1.5,
            vec![touch(egui::TouchPhase::End, 50.0), egui::Event::PointerGone],
        );
        assert!(gone(&e));
    }
}
