//! How long a new NWS alert takes to reach the map (1008.md A1, ROADMAP_PARITY M1.3/M1.4).
//!
//! Three clocks, kept apart:
//!
//! - **sent**: the product's own `sent` time, set by the NWS. It is the issuing office's clock,
//!   not ours.
//! - **received**: the local wall clock when the poll whose reply first carried the message came
//!   back. It includes the time the alert waited for the next poll, which is most of it.
//! - **drawn**: the local wall clock at the end of the first frame built after the app accepted
//!   that reply with the warnings layer on. That is the CPU frame, not the moment a display showed
//!   it; presentation is not measured here and is never called on-screen latency.
//!
//! Only messages that first appear after watching started are measured: an alert already active
//! at the first poll was issued before anyone was looking, so its age says nothing about delivery.
//! Those, and messages without a `sent` time, are counted as excluded, never as zero.
//! "sent → received" compares two different clocks; a skewed local clock shows up as a shift (and
//! can make a sample negative). Samples are kept as measured, not clamped.

use crate::overlay::AlertInfo;
use chrono::{DateTime, Utc};
use std::collections::{HashSet, VecDeque};

/// Samples kept for the percentiles: about a busy afternoon's new warnings and statements.
pub const CAPACITY: usize = 512;

/// One new alert message, as it reached the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The message id (each update or continuation is a new message with its own `sent`).
    pub id: String,
    pub event: String,
    /// The VTEC event this message belongs to, when it has one.
    pub event_key: String,
    pub sent: DateTime<Utc>,
    pub received: DateTime<Utc>,
    /// The first frame built with it on the map; `None` until then, or when the layer was off.
    pub drawn: Option<DateTime<Utc>>,
    /// Where it came from, as the alert card names it.
    pub source: &'static str,
}

impl Sample {
    pub fn sent_to_received_s(&self) -> f64 {
        (self.received - self.sent).num_milliseconds() as f64 / 1000.0
    }

    pub fn received_to_drawn_s(&self) -> Option<f64> {
        self.drawn
            .map(|d| (d - self.received).num_milliseconds() as f64 / 1000.0)
    }

    pub fn sent_to_drawn_s(&self) -> Option<f64> {
        self.drawn
            .map(|d| (d - self.sent).num_milliseconds() as f64 / 1000.0)
    }
}

/// p50/p95 of one stage, in seconds, over `count` samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub count: usize,
    pub p50_s: f64,
    pub p95_s: f64,
}

/// The measured stages and what was left out.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Summary {
    pub sent_to_received: Option<Stage>,
    pub received_to_drawn: Option<Stage>,
    pub sent_to_drawn: Option<Stage>,
    /// Active at the first poll, so issued before watching began.
    pub excluded_already_active: u64,
    /// No usable `sent` time on the message.
    pub excluded_no_sent: u64,
    /// Accepted while the warnings layer was off, so never drawn.
    pub not_drawn: u64,
}

/// The log of new alert messages and when each reached the app.
#[derive(Debug, Default)]
pub struct LatencyLog {
    seeded: bool,
    seen: HashSet<String>,
    samples: VecDeque<Sample>,
    /// Messages received but not yet drawn, oldest first.
    awaiting_draw: Vec<String>,
    excluded_already_active: u64,
    excluded_no_sent: u64,
    not_drawn: u64,
}

impl LatencyLog {
    /// Note one poll's reply, which came back at `received`. The first reply only seeds the log.
    /// Returns how many new messages were sampled.
    pub fn observe<'a>(
        &mut self,
        alerts: impl IntoIterator<Item = &'a AlertInfo>,
        received: DateTime<Utc>,
        source: &'static str,
    ) -> usize {
        let mut added = 0;
        for a in alerts {
            // A multi-part polygon arrives as several features of one message.
            if a.id.is_empty() || !self.seen.insert(a.id.clone()) {
                continue;
            }
            if !self.seeded {
                self.excluded_already_active += 1;
                continue;
            }
            let Some(sent) = a.issued else {
                self.excluded_no_sent += 1;
                continue;
            };
            self.push(Sample {
                id: a.id.clone(),
                event: a.event.clone(),
                event_key: a.event_key(),
                sent,
                received,
                drawn: None,
                source,
            });
            added += 1;
        }
        self.seeded = true;
        added
    }

    fn push(&mut self, sample: Sample) {
        self.awaiting_draw.push(sample.id.clone());
        self.samples.push_back(sample);
        while self.samples.len() > CAPACITY {
            if let Some(old) = self.samples.pop_front() {
                self.awaiting_draw.retain(|id| *id != old.id);
            }
        }
    }

    /// The first frame after the reply was accepted finished at `at`. With `layer_on` false the
    /// waiting messages were never put on the map and are counted as not drawn.
    pub fn frame_built(&mut self, at: DateTime<Utc>, layer_on: bool) {
        if self.awaiting_draw.is_empty() {
            return;
        }
        let waiting = std::mem::take(&mut self.awaiting_draw);
        if !layer_on {
            self.not_drawn += waiting.len() as u64;
            return;
        }
        for s in self.samples.iter_mut().filter(|s| waiting.contains(&s.id)) {
            s.drawn = Some(at);
        }
    }

    /// Whether the first reply has been seen (and with it, what was already active).
    pub fn is_seeded(&self) -> bool {
        self.seeded
    }

    /// Whether any received message still waits for its first frame.
    pub fn awaiting_draw(&self) -> bool {
        !self.awaiting_draw.is_empty()
    }

    pub fn samples(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter()
    }

    /// The stages over the samples `keep` accepts (all of them, warnings only, ...).
    pub fn summary(&self, keep: impl Fn(&Sample) -> bool) -> Summary {
        let kept: Vec<&Sample> = self.samples.iter().filter(|s| keep(s)).collect();
        Summary {
            sent_to_received: stage(kept.iter().map(|s| Some(s.sent_to_received_s()))),
            received_to_drawn: stage(kept.iter().map(|s| s.received_to_drawn_s())),
            sent_to_drawn: stage(kept.iter().map(|s| s.sent_to_drawn_s())),
            excluded_already_active: self.excluded_already_active,
            excluded_no_sent: self.excluded_no_sent,
            not_drawn: self.not_drawn,
        }
    }
}

/// p50/p95 (nearest rank) of the present values; `None` when there are none.
pub fn stage(values: impl IntoIterator<Item = Option<f64>>) -> Option<Stage> {
    let mut v: Vec<f64> = values.into_iter().flatten().collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let rank = |q: f64| v[((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    Some(Stage {
        count: v.len(),
        p50_s: rank(0.50),
        p95_s: rank(0.95),
    })
}

/// Whether `event` is a warning: the products the latency matters most for.
pub fn is_warning(event: &str) -> bool {
    event.ends_with("Warning")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(id: &str, event: &str, sent: Option<i64>) -> AlertInfo {
        AlertInfo {
            id: id.into(),
            event: event.into(),
            headline: String::new(),
            area: String::new(),
            description: String::new(),
            instruction: String::new(),
            expires: None,
            issued: sent.map(|s| DateTime::from_timestamp(s, 0).unwrap()),
            effective: None,
            max_hail_in: None,
            max_wind: None,
            tornado_detection: None,
            damage_threat: None,
            source: None,
            motion: None,
            vtec: None,
        }
    }

    fn at(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(s, 0).unwrap()
    }

    #[test]
    fn only_messages_new_since_the_first_poll_are_measured() {
        let mut log = LatencyLog::default();
        let old = alert("a", "Tornado Warning", Some(0));
        assert_eq!(
            log.observe([&old], at(100), "api"),
            0,
            "the first reply seeds"
        );
        let new = alert("b", "Tornado Warning", Some(150));
        let unsent = alert("c", "Flood Advisory", None);
        // The same message twice (two polygon parts) is one sample.
        assert_eq!(log.observe([&old, &new, &new, &unsent], at(220), "api"), 1);
        let s = log.summary(|_| true);
        assert_eq!(s.excluded_already_active, 1);
        assert_eq!(s.excluded_no_sent, 1);
        let r = s.sent_to_received.unwrap();
        assert_eq!((r.count, r.p50_s), (1, 70.0));
        assert!(s.sent_to_drawn.is_none(), "not drawn yet is not zero");
        log.frame_built(at(221), true);
        let s = log.summary(|_| true);
        assert_eq!(s.received_to_drawn.unwrap().p50_s, 1.0);
        assert_eq!(s.sent_to_drawn.unwrap().p50_s, 71.0);
        assert!(!log.awaiting_draw());
    }

    #[test]
    fn a_reply_accepted_with_the_layer_off_is_not_drawn() {
        let mut log = LatencyLog::default();
        log.observe([], at(0), "api");
        log.observe(
            [&alert("x", "Severe Thunderstorm Warning", Some(10))],
            at(40),
            "api",
        );
        log.frame_built(at(41), false);
        let s = log.summary(|_| true);
        assert_eq!(s.not_drawn, 1);
        assert!(s.received_to_drawn.is_none());
        // A later frame with the layer on does not draw it after the fact.
        log.frame_built(at(90), true);
        assert!(log.summary(|_| true).sent_to_drawn.is_none());
    }

    #[test]
    fn percentiles_are_nearest_rank_and_keep_negative_skew() {
        let st = stage((1..=20).map(|i| Some(i as f64))).unwrap();
        assert_eq!((st.count, st.p50_s, st.p95_s), (20, 10.0, 19.0));
        let skew = stage([Some(-2.0), None, Some(5.0)]).unwrap();
        assert_eq!((skew.count, skew.p50_s), (2, -2.0));
        assert!(stage([None, None]).is_none());
    }

    #[test]
    fn the_summary_can_be_limited_to_warnings() {
        let mut log = LatencyLog::default();
        log.observe([], at(0), "api");
        log.observe(
            [
                &alert("w", "Tornado Warning", Some(0)),
                &alert("s", "Special Weather Statement", Some(0)),
            ],
            at(30),
            "api",
        );
        let w = log.summary(|s| is_warning(&s.event));
        assert_eq!(w.sent_to_received.unwrap().count, 1);
        assert_eq!(log.summary(|_| true).sent_to_received.unwrap().count, 2);
    }

    #[test]
    fn the_log_is_bounded() {
        let mut log = LatencyLog::default();
        log.observe([], at(0), "api");
        let many: Vec<AlertInfo> = (0..CAPACITY + 10)
            .map(|i| alert(&format!("m{i}"), "Flood Warning", Some(0)))
            .collect();
        log.observe(&many, at(60), "api");
        assert_eq!(log.samples().count(), CAPACITY);
        log.frame_built(at(61), true);
        assert!(log.samples().all(|s| s.drawn.is_some()));
    }
}
