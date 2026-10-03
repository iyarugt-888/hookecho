//! Delivery of radar data messages (volumes, frame lists, live chunks and progress) into the panes.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn poll_messages(&mut self) {
        self.drain_feed_errors();
        while let Ok(msg) = self.msg_rx.try_recv() {
            let idx = msg.view();
            // LiveEnded must be handled even after a site change (to drop the stream handle).
            if let DataMsg::LiveEnded {
                view, gen, lost, ..
            } = msg
            {
                self.live_ended(view, gen, lost);
                continue;
            }
            if idx >= self.views.len() || self.views[idx].site.as_deref() != Some(msg.site()) {
                continue; // view gone or its site changed since the fetch spawned
            }
            if let DataMsg::Live { gen, .. } | DataMsg::LiveProgress { gen, .. } = &msg {
                if !self
                    .live_stream
                    .as_ref()
                    .is_some_and(|(v, _, g, _)| *v == idx && g == gen)
                {
                    continue; // a superseded provider must never rewind this pane
                }
            }
            match msg {
                DataMsg::Volume {
                    view,
                    name,
                    time,
                    scan,
                    live_poll,
                    ..
                } => {
                    let scan = Arc::new(scan);
                    self.scan_cache.put(name.clone(), Arc::clone(&scan));
                    let v = &mut self.views[view];
                    // A completed fetch may belong to a cursor the user has since left. Keep
                    // the scan in cache for a later revisit, but never paint it over the frame
                    // now selected (including a late live poll after an archive scrub).
                    if !v.timeline.accepts_fetched_volume(&name, live_poll) {
                        v.loading = false;
                        continue;
                    }
                    let previous_provider = v.live_scan.provider.clone();
                    let reason = v.live_scan.poll_reason();
                    if live_poll
                        && v.timeline.following
                        && !v.live_scan.accept_volume(&name, time, Utc::now())
                    {
                        v.loading = false;
                        continue;
                    }
                    if live_poll && v.timeline.following {
                        if let Some(from) = previous_provider
                            .filter(|from| v.live_scan.provider.as_deref() != Some(from.as_str()))
                        {
                            log::info!(
                                target: "hookecho::radar_provider_manager",
                                "{}: provider switch {from} -> Completed-volume poll: {reason}",
                                v.site.as_deref().unwrap_or("?"),
                            );
                            v.live_scan.set_switch_reason(reason);
                            v.live_render_started = None;
                            v.live_queue_timings =
                                Arc::new(crate::render::LiveQueueTimings::default());
                        }
                    }
                    let looping = v.timeline.live_looping();
                    // A newly-arrived live head (following): roll the day at UTC midnight, or grow
                    // the frame list so the loop window slides forward. A frame-fetch result for a
                    // scrubbed/loop-display frame is older than the head and isn't a new head.
                    //
                    // This is *not* the same question as `live_poll`: the bucket listing
                    // (`DataMsg::Frames`) can independently learn this exact name before this
                    // fetch completes, so "new to `t.frames`" and "came from the live poll" can
                    // disagree — both checks exist because each answers a different question.
                    let new_head = v.timeline.following
                        && v.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
                        && {
                            let last_time = v.timeline.frames.last().and_then(|id| id.date_time());
                            if time.date_naive() != v.timeline.date {
                                v.timeline.date = time.date_naive(); // re-list fires via frames_key
                                true
                            } else if last_time.is_none_or(|t| time > t)
                                && v.timeline.frames.last().map(|id| id.name())
                                    != Some(name.as_str())
                            {
                                v.timeline.append_head(Identifier::new(name.clone()));
                                true
                            } else {
                                false
                            }
                        };
                    log::debug!(
                        target: "hookecho::live_sweep",
                        "{}: volume loaded {name} ({time}), live_poll={live_poll}",
                        v.site.as_deref().unwrap_or("?"),
                    );
                    // While looping, the playhead frame owns the display: a new head is only
                    // appended, and a poll that returns the head already listed must not paint
                    // it over an older loop frame either (it did, every poll, so a live loop kept
                    // snapping to its newest scan between frames). Every other case updates the
                    // displayed volume.
                    if !looping
                        || (!new_head && v.timeline.current().is_some_and(|id| id.name() == name))
                    {
                        v.show_volume(scan, name, time);
                    }
                    // A stale in-flight poll completing after the user scrubbed away from live is
                    // simply not recorded — `following` is checked at the only point that matters,
                    // when the result actually lands, rather than trusted from when the fetch
                    // started.
                    if live_poll && v.timeline.following {
                        v.last_live_arrival = Some((Utc::now(), time));
                    }
                    v.loading = false;
                    v.error = None;
                    v.clamp_tilt();
                    v.clamp_moment();
                    self.pane_shown.remove(&view);
                    self.scan_chime(view, time);
                }
                DataMsg::Frames {
                    view,
                    site,
                    date,
                    frames,
                } => {
                    let v = &mut self.views[view];
                    if v.timeline.date == date && v.site.as_deref() == Some(site.as_str()) {
                        v.timeline.listing = false;
                        v.timeline.set_frames(frames, (site, date));
                        self.pane_shown.remove(&view);
                    }
                }
                DataMsg::Live {
                    view,
                    name,
                    time,
                    received_at,
                    radial_coverage,
                    scan,
                    changed,
                    retries,
                    decode_time,
                    ..
                } => {
                    let v = &mut self.views[view];
                    if v.timeline.playing {
                        continue; // looping pane owns its displayed frame (cf. Volume above)
                    }
                    if !v.live_scan.accept_volume(&name, time, Utc::now()) {
                        continue; // late chunk from an older volume cannot reverse display time
                    }
                    let acquisition = radial_coverage
                        .and_then(|coverage| v.live_scan.capture_acquisition(coverage, Utc::now()));
                    v.live_render_started = received_at;
                    log::debug!(
                        target: "hookecho::live_sweep",
                        "{}: live chunk merged into {name} ({time}), {} tilt(s) changed, \
                         decode {:.0} ms, {retries} retr{}",
                        v.site.as_deref().unwrap_or("?"),
                        changed.len(),
                        decode_time.as_secs_f64() * 1000.0,
                        if retries == 1 { "y" } else { "ies" },
                    );
                    match &mut v.volume {
                        Some(vol) => {
                            vol.apply_live_captured(scan, name, time, &changed, acquisition)
                        }
                        None => {
                            v.volume =
                                Some(Volume::from_live_captured(scan, name, time, acquisition))
                        }
                    }
                    v.live_scan_revision = v.live_scan_revision.wrapping_add(1);
                    // Phase B5's "follow newest low-level cut": jump to the lowest tilt the
                    // instant a sweep there lands, including a SAILS/MRLE mid-volume rescan,
                    // rather than waiting for the tilt already selected or the volume as a whole.
                    if v.follow_lowest_cut
                        && v.timeline.following
                        && v.volume
                            .as_ref()
                            .is_some_and(|vol| vol.changed_includes_lowest_tilt(&changed))
                    {
                        v.tilt = 0;
                    }
                    // Follow the live sweep: when a new sweep's first chunk has merged (so its
                    // tilt exists to show), move to it — once per sweep, so a tilt picked by hand
                    // mid-sweep holds until the radar starts the next one.
                    v.follow_sweep();
                    v.last_live_arrival = Some((Utc::now(), time));
                    // `LiveProgress` immediately precedes this partial merge. Keep it: the
                    // scrubber and 2D sweep bar need to describe/animate the chunk now on screen.
                    // Stream end and site changes clear it, so it cannot linger indefinitely.
                    v.live_retries = retries;
                    v.last_decode_time = Some(decode_time);
                    let now = Utc::now();
                    v.live_history.push_back((
                        now,
                        (now - time).num_milliseconds() as f32 / 1000.0,
                        decode_time.as_secs_f32() * 1000.0,
                    ));
                    while v.live_history.len() > crate::view::LIVE_HISTORY {
                        v.live_history.pop_front();
                    }
                    v.loading = false;
                    v.error = None;
                    v.clamp_tilt();
                    v.clamp_moment();
                    // A healthy stream pushes the poll deadline forward — this line IS the
                    // fallback: if the stream dies, interval polling resumes on schedule.
                    v.last_poll = Some(Instant::now());
                    self.pane_shown.remove(&view);
                    self.scan_chime(view, time);
                }
                DataMsg::UpToDate { view, .. } => self.views[view].loading = false,
                DataMsg::Prefetched {
                    view, name, scan, ..
                } => {
                    if view < self.views.len() {
                        self.remember_light(view, &name, &scan);
                    }
                    self.scan_cache.put(name, Arc::new(scan));
                }
                DataMsg::Error { view, err, .. } => {
                    let v = &mut self.views[view];
                    v.loading = false;
                    // The newest archive volume is published while the radar is still writing it,
                    // so the head can briefly lack its VCP message. That is a "not finished yet",
                    // not a failure: the next poll gets a complete file a minute later. Showing a
                    // red chip for it left the map looking broken while nothing was wrong.
                    if err.contains("missing coverage pattern") {
                        log::debug!("head volume not complete yet: {err}");
                    } else {
                        v.error = Some(err);
                    }
                }
                DataMsg::LiveProgress { view, progress, .. } => {
                    self.views[view].live_progress = Some(progress);
                    self.views[view].live_progress_at = Some(Instant::now());
                    self.views[view].live_scan.progress(progress, Utc::now());
                }
                DataMsg::LiveEnded { .. } => unreachable!("handled above"),
            }
        }
    }
}
