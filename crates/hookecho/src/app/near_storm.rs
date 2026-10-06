//! The HRRR hour beside the radar, for the Tornado ID's environment gate (`wxdata::near_storm`,
//! detectionplan.md "The near-storm environment"): a Possible verdict in air whose inflow STP is
//! under `near_storm::GATE_STP` is not shown.
//!
//! One hour is held, the one the active volume falls in, fetched as `near_storm::fetch_hour` does
//! for the backtest (the run an hour before at F+1, else the on-hour analysis or an older run), so
//! the live gate reads the same air the measurement did. Until it arrives, or when it cannot be
//! had, verdicts are drawn without the gate, as they always were.

use super::*;
use std::sync::{mpsc, Arc};
use wxdata::near_storm::EnvHour;

/// How long (s) before an hour that could not be had is asked for again: a live volume's run may
/// not be posted yet.
const RETRY_S: f64 = 600.0;

/// The HRRR hour the active volume's verdicts read.
#[derive(Default)]
pub(crate) struct NearStormFeed {
    /// The valid hour (seconds since the epoch) last fetched, what came of it (the run it came
    /// from and the hour; `None`: could not be had), and when (egui time, s).
    have: Option<(i64, Option<Source>, f64)>,
    /// The fetch in flight, for its valid hour.
    pending: Option<(i64, mpsc::Receiver<Option<Source>>)>,
}

/// An HRRR hour and the run it came from.
type Source = (chrono::DateTime<chrono::Utc>, Arc<EnvHour>);

impl NearStormFeed {
    /// The hour valid at `valid`, if it is the one held.
    pub(crate) fn hour(&self, valid: i64) -> Option<Arc<EnvHour>> {
        self.source(valid).map(|(_, h)| h)
    }

    /// [`Self::hour`], with the run it came from.
    pub(crate) fn source(&self, valid: i64) -> Option<Source> {
        match &self.have {
            Some((v, Some(s), _)) if *v == valid => Some(s.clone()),
            _ => None,
        }
    }
}

/// The HRRR valid hour a volume scanned at `time` reads: the hour it falls in.
pub(crate) fn valid_hour(time: chrono::DateTime<chrono::Utc>) -> i64 {
    time.timestamp().div_euclid(3600) * 3600
}

impl HookEchoApp {
    /// The HRRR hour for pane `idx`'s volume, asking for it when that hour is not held. `None`
    /// until it arrives, and when it cannot be had.
    pub(crate) fn near_storm_hour(
        &mut self,
        idx: usize,
        ctx: &egui::Context,
    ) -> Option<Arc<EnvHour>> {
        let valid = valid_hour(self.views[idx].volume.as_ref()?.time);
        let now = ctx.input(|i| i.time);
        let feed = &mut self.near_storm;
        if let Some((v, rx)) = &feed.pending {
            match rx.try_recv() {
                Ok(source) => {
                    feed.have = Some((*v, source, now));
                    feed.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => feed.pending = None,
            }
        }
        match &feed.have {
            Some((v, Some((_, h)), _)) if *v == valid => return Some(h.clone()),
            Some((v, None, at)) if *v == valid && now - at < RETRY_S => return None,
            _ => {}
        }
        if feed.pending.as_ref().is_some_and(|(v, _)| *v == valid) {
            return None;
        }
        let when = chrono::DateTime::from_timestamp(valid, 0)?;
        let (tx, rx) = mpsc::channel();
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let hour = match wxdata::near_storm::fetch_hour(&http, when).await {
                Ok((run, hour)) => Some((run, Arc::new(hour))),
                Err(e) => {
                    log::debug!("near-storm environment for {when}: {e:#}");
                    None
                }
            };
            let _ = tx.send(hour);
            ctx.request_repaint();
        });
        // A fetch for another hour still in flight is dropped: its answer is no longer wanted.
        self.near_storm.pending = Some((valid, rx));
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_volume_reads_the_hour_it_falls_in() {
        let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().to_utc();
        assert_eq!(
            super::valid_hour(at("2024-05-06T23:59:59Z")),
            at("2024-05-06T23:00:00Z").timestamp()
        );
        assert_eq!(
            super::valid_hour(at("2024-05-07T00:00:00Z")),
            at("2024-05-07T00:00:00Z").timestamp()
        );
    }
}
