//! Warnings watched and told: new-warning detection, the alert sounds and notifications, the
//! alert badge and popup, archived warnings for a scrubbed view, and the warning history at a point.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Every alert sound goes through here, so one mute switch covers all of them (and any that
    /// get added later) instead of a guard per call site.
    pub(crate) fn play_alert(&self, sound: &crate::settings::AlertSound) {
        if self.settings.mute_alerts || self.in_quiet_hours() {
            return;
        }
        crate::audio::play(sound, self.settings.alert_volume);
        crate::platform::haptic(crate::platform::Haptic::Alert);
    }

    /// A sound quiet hours does not silence: the escalated warning tiers and the two detections
    /// that mean a tornado may be on the ground. `mute_alerts` still wins — that switch is the
    /// user saying so about right now, where quiet hours is a standing preference.
    pub(crate) fn play_alert_urgent(&self, sound: &crate::settings::AlertSound) {
        if self.settings.mute_alerts {
            return;
        }
        crate::audio::play(sound, self.settings.alert_volume);
        crate::platform::haptic(crate::platform::Haptic::Alert);
    }

    /// Detect warning-tier alerts whose id we haven't seen, raising a banner + audible cue for
    /// each new one. The first fetch only seeds the known set (no alert on already-active warnings).
    pub(crate) fn detect_new_warnings(&mut self, feats: &[GeoFeature]) {
        let metric = self.metric();
        let mut alerted = false;
        let mut max_esc = 0u8; // highest escalation among newly-seen warnings this pass
                               // Collected, not spoken here: the tone has to play first, and it plays once for the whole
                               // pass rather than once per warning.
        let mut to_speak: Vec<(u8, String)> = Vec::new();
        // Only banner warnings within the selected radar's coverage — a warning covering a saved
        // location still banners + pushes regardless (that's a watched place, not the viewed site).
        let site_box = self.active_site_bounds(250.0);
        for f in feats {
            if f.kind != overlay::FeatureKind::Warning {
                continue;
            }
            let Some(a) = &f.alert else { continue };
            // Mark every warning seen so it can't re-banner later, but only alert on genuinely new
            // ones after the first (seeding) pass. Keyed by VTEC event, not message id — an office
            // re-issues a continuation of the same warning every few minutes with a fresh id, and
            // deduping on that is why the same tornado warning announced itself over and over.
            if self.known_warning_ids.insert(a.dedupe_key()) && self.warnings_seeded {
                let esc = wxdata::alerts::escalation(a);
                let urgent = esc >= 2;
                // The polygon's middle, computed once: the rule pass below uses it as the
                // warning's stand-in detection, and the spoken line uses it to say which way the
                // warning lies from a watched place.
                let centroid: Option<[f64; 2]> =
                    f.rings.first().filter(|r| !r.is_empty()).map(|ring| {
                        let n = ring.len() as f64;
                        let (x, y) = ring
                            .iter()
                            .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
                        [x / n, y / n]
                    });
                // Severity floor: below the tier the user set, the warning still banners and
                // still joins the alert list — it just doesn't push, speak or make noise.
                let notify_ok = esc >= self.settings.alert_min_escalation;
                // A watched location always alerts + pushes: inside the polygon, or within that
                // marker's radius of it. Home first, then the closest — a warning that clips two
                // saved places should name the one you sleep in.
                // Owned, not borrowed: the rule pass below needs `&mut self` while this is
                // still in hand.
                let hit: Option<(String, f64, Option<f32>)> = self
                    .settings
                    .markers
                    .iter()
                    .filter_map(|m| {
                        let km = f.distance_km(m.lon, m.lat);
                        (km <= m.alert_radius_mi * crate::geo::KM_PER_MILE).then(|| {
                            // Distance to the nearest edge, bearing to the middle: the edge is
                            // what "how far" means for a polygon, and the middle is what "which
                            // way" means. Mixing them is fine at the scale of one sentence.
                            let bearing = centroid
                                .map(|c| crate::geo::great_circle([m.lon, m.lat], c).1 as f32);
                            (m.name.clone(), m.home, km, bearing)
                        })
                    })
                    .min_by(|(_, ha, ka, _), (_, hb, kb, _)| {
                        hb.cmp(ha)
                            .then(ka.partial_cmp(kb).unwrap_or(std::cmp::Ordering::Equal))
                    })
                    .map(|(name, _, km, bearing)| (name, km, bearing));
                // A drawn watch zone the warning polygon touches. Independent of the markers: a
                // zone is an area you care about, not a point with a radius around it.
                let zone: Option<String> = self
                    .settings
                    .alert_polygons
                    .iter()
                    .find(|z| {
                        f.rings
                            .first()
                            .is_some_and(|outer| wxdata::overlay::rings_intersect(outer, &z.ring))
                    })
                    .map(|z| z.name.clone());
                if let (Some(z), true) = (zone.as_deref(), notify_ok) {
                    self.notify_alert(
                        &format!("⚠ {} — {z}", a.event),
                        if a.headline.is_empty() {
                            &a.area
                        } else {
                            &a.headline
                        },
                        urgent,
                    );
                }
                // User rules that watch warnings. The id-dedupe above is their cooldown: a
                // warning is announced once, however long it stands.
                for rule in self.settings.alert_rules.clone() {
                    if !rule.enabled || !crate::rules::warning_matches(&rule, &a.event) {
                        continue;
                    }
                    // Anywhere: the warning polygon is somewhere on this radar. A place: the
                    // polygon has to reach it, using the same distance the built-in alert uses.
                    let reaches = match &rule.place {
                        crate::settings::RulePlace::Anywhere => true,
                        crate::settings::RulePlace::Marker { id } => self
                            .settings
                            .markers
                            .iter()
                            .find(|m| &m.id == id)
                            .is_some_and(|m| {
                                f.distance_km(m.lon, m.lat)
                                    <= m.alert_radius_mi * crate::geo::KM_PER_MILE
                            }),
                        crate::settings::RulePlace::Zone { name } => self
                            .settings
                            .alert_polygons
                            .iter()
                            .find(|z| &z.name == name)
                            .is_some_and(|z| {
                                f.rings.first().is_some_and(|outer| {
                                    wxdata::overlay::rings_intersect(outer, &z.ring)
                                })
                            }),
                    };
                    if reaches {
                        // The warning's own centroid stands in for a detection, so a warning rule
                        // can carry extra conditions like every other rule ("a tornado warning,
                        // and also rotation within 20 km").
                        let hit = centroid
                            .map(|c| crate::rules::Detection::at(c[0], c[1]))
                            .unwrap_or(crate::rules::Detection::at(0.0, 0.0));
                        if crate::rules::compound_ok(&rule, &hit, &self.recent_for_rules()) {
                            self.fire_rule_named(&rule, &hit, Some(a.event.clone()));
                        }
                    }
                }
                let zone_name = zone;
                // `area` is banner text and `relation` is speech: "5 mi from Home" reads as
                // "five em eye", and "covers Home" reads as a verb the sentence already had.
                let (label, area, relation) = match hit {
                    Some((name, km, bearing)) => {
                        // Watched location covered → push to the phone (opt-in ntfy topic).
                        if notify_ok {
                            self.notify_alert(
                                &format!("⚠ {} — {}", a.event, name),
                                if a.headline.is_empty() {
                                    &a.area
                                } else {
                                    &a.headline
                                },
                                urgent,
                            );
                        }
                        let where_ = if km <= 0.05 {
                            format!("covers {name}")
                        } else {
                            format!("{} from {name}", crate::geo::fmt_distance(km, metric, 0))
                        };
                        (
                            format!("⚠ {}", a.event),
                            where_,
                            wxdata::spoken::relation(&name, km, bearing, metric),
                        )
                    }
                    // A zone hit banners on its own terms, wherever the radar happens to be
                    // pointed — that is the whole point of drawing one.
                    None if zone_name.is_some() => {
                        let zone = zone_name.expect("checked Some");
                        (
                            format!("⚠ {}", a.event),
                            format!("touches {zone}"),
                            format!("touching {zone}"),
                        )
                    }
                    None => {
                        // No watched location: banner only if it's near the selected radar.
                        if site_box.is_none_or(|bx| !feature_in_box(f, bx)) {
                            continue;
                        }
                        // Nowhere of the user's own to relate it to; the counties carry it.
                        (a.event.clone(), a.area.clone(), String::new())
                    }
                };
                if notify_ok {
                    max_esc = max_esc.max(esc);
                }
                // Queued, not spoken: the tone leads, and the whole pass is announced together
                // below so two warnings in one fetch cannot talk over each other. Chasing is an
                // eyes-on-the-road activity, and a warning you have to read is one you read late.
                if self.settings.speak_warnings && notify_ok {
                    let until = a
                        .expires
                        .map(|t| {
                            crate::timefmt::fmt_clock(
                                t,
                                self.settings
                                    .tz_for(self.views[self.active].site.as_deref()),
                                false,
                            )
                        })
                        .unwrap_or_default();
                    // Hazard, then where it sits against a place you know, then the counties, the
                    // towns in its path and what to do — see `wxdata::spoken`.
                    to_speak.push((esc, wxdata::spoken::warning_script(a, &relation, &until)));
                }
                if notify_ok && self.settings.ntfy_snapshot {
                    // Newest wins: one picture per pass, of whatever last warned.
                    self.snapshot_push = Some(format!("{label} — {area}"));
                }
                self.banner(label, area);
                alerted |= notify_ok;
            }
        }
        self.warnings_seeded = true;
        if alerted {
            print!("\x07"); // free terminal bell alongside the chime
            use std::io::Write;
            let _ = std::io::stdout().flush();
            // Escalated (Tornado Emergency / PDS / destructive) warnings use the emergency sound
            // and go past quiet hours — which is now true of the words as well as the tone. The
            // voice used to ignore quiet hours entirely, so a 3 a.m. warning too minor to chime
            // for still read itself out in the dark.
            let urgent = max_esc >= 2;
            if !self.settings.mute_alerts && (urgent || !self.in_quiet_hours()) {
                let tone = self.settings.alert_sound.then(|| {
                    (
                        if urgent {
                            self.settings.emergency_sound.clone()
                        } else {
                            self.settings.warn_sound.clone()
                        },
                        self.settings.alert_volume,
                    )
                });
                if tone.is_some() {
                    crate::platform::haptic(crate::platform::Haptic::Alert);
                }
                // The voice tracks the same slider the tones do; Piper's output has no level of
                // its own, so without this the words arrived louder than the tone.
                crate::speech::set_volume(self.settings.alert_volume);
                // One announcement for the whole pass: highest escalation first, then the rest.
                to_speak.sort_by_key(|(esc, _)| std::cmp::Reverse(*esc));
                crate::speech::announce(
                    if urgent {
                        crate::speech::Priority::Emergency
                    } else {
                        crate::speech::Priority::Warning
                    },
                    tone,
                    to_speak.into_iter().map(|(_, line)| line).collect(),
                );
            }
        }
    }

    /// Deliver an alert to every configured channel: ntfy.sh push plus Discord / Slack / Matrix
    /// webhooks. Each is a no-op when its settings field is blank.
    /// Best-effort on the shared tokio runtime; failures are logged, never fatal.
    pub(crate) fn notify_alert(&self, title: &str, body: &str, urgent: bool) {
        // Quiet hours hold everything back except the escalated tier, which is the one worth
        // waking up for. Banners and the alert list are untouched — this gates what leaves the
        // machine and what makes noise, not what the app knows.
        if !urgent && self.in_quiet_hours() {
            log::debug!("quiet hours: holding push {title:?}");
            if let Ok(mut q) = self.quiet_queue.lock() {
                if q.len() < QUIET_QUEUE_MAX {
                    q.push((title.to_string(), body.to_string()));
                }
            }
            return;
        }
        let http = self.http.clone();
        let (mut title, mut body) = (title.to_string(), body.to_string());

        // Outbreak mode: past the threshold, one rolling summary goes out instead of one push per
        // warning. Escalated alerts are exempt — those are the ones worth a buzz each.
        // ponytail: the summary is a fresh notification each refresh, not an in-place replace;
        // desktop replace-by-tag and the Android fixed notification id can land with the rest of
        // the Android delivery stack.
        if !urgent && self.settings.alert_rollup_threshold > 0 {
            let window =
                std::time::Duration::from_secs(self.settings.alert_rollup_window_min.max(1) * 60);
            let decision = self.rollup.lock().map(|mut r| {
                r.offer(
                    Instant::now(),
                    &title,
                    self.settings.alert_rollup_threshold,
                    window,
                )
            });
            match decision {
                Ok(crate::alert_rollup::Decision::Hold) => return,
                Ok(crate::alert_rollup::Decision::Rollup(text)) => {
                    title = "Multiple alerts".to_string();
                    body = text;
                }
                _ => {}
            }
        }
        let (title, body) = (title, body);

        if self.settings.desktop_notify {
            crate::notify::desktop(&title, &body);
        }

        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        crate::mqtt::publish_alert(&self.settings, &title, &body, urgent);

        let topic = self.settings.ntfy_topic.trim().to_string();
        if !topic.is_empty() {
            let (http, title, body) = (http.clone(), title.clone(), body.clone());
            let priority = if urgent { "urgent" } else { "high" };
            self.spawner.spawn(async move {
                crate::notify::send_retrying("ntfy push", || {
                    http.post(format!("https://ntfy.sh/{topic}"))
                        .header("Title", title.clone())
                        .header("Priority", priority)
                        .header("Tags", "warning,cloud_with_lightning")
                        .body(body.clone())
                })
                .await;
            });
        }

        // Chat webhooks: same shape (POST JSON), so one closure covers Discord and Slack.
        let mut posts: Vec<(&'static str, String, String, Option<String>)> = Vec::new();
        let discord = self.settings.discord_webhook.trim();
        if !discord.is_empty() {
            posts.push((
                "discord",
                discord.to_string(),
                crate::notify::discord_body(&title, &body),
                None,
            ));
        }
        let slack = self.settings.slack_webhook.trim();
        if !slack.is_empty() {
            posts.push((
                "slack",
                slack.to_string(),
                crate::notify::slack_body(&title, &body),
                None,
            ));
        }
        for (what, url, payload, _) in posts {
            let http = http.clone();
            self.spawner.spawn(async move {
                crate::notify::send_retrying(&format!("{what} webhook"), || {
                    http.post(url.clone())
                        .header("Content-Type", "application/json")
                        .body(payload.clone())
                })
                .await;
            });
        }

        // Matrix wants an authenticated PUT with a transaction id.
        let (hs, room, token) = (
            self.settings.matrix_homeserver.trim().to_string(),
            self.settings.matrix_room.trim().to_string(),
            self.settings.matrix_token.trim().to_string(),
        );
        if !hs.is_empty() && !room.is_empty() && !token.is_empty() {
            let txn = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let url = crate::notify::matrix_url(&hs, &room, txn);
            let payload = crate::notify::matrix_body(&title, &body);
            self.spawner.spawn(async move {
                crate::notify::send_retrying("matrix webhook", || {
                    http.put(url.clone())
                        .bearer_auth(token.clone())
                        .header("Content-Type", "application/json")
                        .body(payload.clone())
                })
                .await;
            });
        }
    }

    /// Fetch the archived NWS warning history for `(lon, lat)`. Cheap (one JSON request), so it runs
    /// per click rather than being cached; the IEM archive starts in 1986.
    pub(crate) fn query_warning_history(&mut self, lon: f64, lat: f64) {
        self.climo_warn = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.climo_warn_rx = Some(rx);
        let http = self.http.clone();
        let edate = chrono::Utc::now().format("%Y-%m-%d").to_string();
        self.spawner.spawn(async move {
            let res =
                wxdata::archive_warnings::fetch_point_events(&http, lon, lat, "1986-01-01", &edate)
                    .await
                    .map(|e| wxdata::archive_warnings::summarize(&e))
                    .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Open the warning popup on the alert with `id` (from the alerts panel), showing its bulletin.
    /// Floating Layers panel (desktop): a right-edge glass card holding the searchable registry.
    /// Android hosts the same body in the quick-layers sheet (see `app::mobile`).
    /// Active alerts in the current view — the bell's badge count and its urgency colour.
    pub(crate) fn alert_badge(&mut self) -> (usize, u8) {
        let bounds = self.view_bounds();
        let rows = crate::ui::alert_panel::rows_in_view(self.active_alert_features(), bounds);
        let max_esc = rows.iter().map(|r| r.esc).max().unwrap_or(0);
        (rows.len(), max_esc)
    }

    /// The time alert statuses are told against: the scrubbed frame's while archived warnings
    /// are shown (an archived warning was in effect then, however long ago), else `None`, the
    /// clock.
    pub(crate) fn alerts_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.arch_warn_shown?;
        self.views[self.active].volume.as_ref().map(|v| v.time)
    }

    pub(crate) fn open_alert_popup(&mut self, id: &str) {
        let mut seen = std::collections::HashSet::new();
        let cards: Vec<ui::warning_window::WarnCard> = self
            .active_alert_features()
            .iter()
            .filter_map(|f| f.alert.as_ref().map(|a| (a, f.stroke)))
            .filter(|(a, _)| a.id == id && seen.insert(a.id.clone()))
            .map(|(a, color)| ui::warning_window::WarnCard {
                info: a.clone(),
                color,
            })
            .collect();
        if !cards.is_empty() {
            self.detail = None;
            self.warning_popup = Some(ui::warning_window::WarningPopup {
                cards,
                selected: Some(0),
                at: self.alerts_at(),
            });
        }
    }

    /// The alert features to display right now: live alerts, or the archived set while the active
    /// pane is scrubbed off-live to a bucket we've fetched (feature W).
    pub(crate) fn active_alert_features(&self) -> &[GeoFeature] {
        if let Some(b) = self.arch_warn_shown {
            if let Some(f) = self.arch_warns.peek(&b) {
                return f;
            }
        }
        &self.alert_features
    }

    /// Drive the archived-warning overlay from the active pane's playhead: fetch the bucket the
    /// scrubbed frame falls in, and swap it in for the live alerts (or back to live at the head).
    pub(crate) fn sync_archive_warnings(&mut self, ctx: &egui::Context) {
        // Discussions follow the same bucket, when their layer is on.
        let md_bucket = self.archive_bucket().filter(|_| self.filters.show_mds);
        match md_bucket {
            None => {
                if self.arch_md_shown.take().is_some() {
                    self.rebuild_overlays();
                }
            }
            Some(b) => {
                let cached = self.arch_mds.contains(&b);
                if !cached && self.arch_md_inflight != Some(b) {
                    self.arch_md_inflight = Some(b);
                    self.spawn_overlay(ctx, OverlaySource::ArchiveMds(b));
                }
                if cached && self.arch_md_shown != Some(b) {
                    self.arch_md_shown = Some(b);
                    self.rebuild_overlays();
                }
            }
        }
        match self.archive_bucket() {
            None => {
                if self.arch_warn_shown.is_some() {
                    self.arch_warn_shown = None;
                    self.rebuild_overlays();
                }
            }
            Some(b) => {
                let cached = self.arch_warns.contains(&b);
                if !cached && self.arch_warn_inflight != Some(b) {
                    self.arch_warn_inflight = Some(b);
                    self.spawn_overlay(ctx, OverlaySource::ArchiveWarnings(b));
                }
                if cached && self.arch_warn_shown != Some(b) {
                    self.arch_warn_shown = Some(b);
                    self.rebuild_overlays();
                }
            }
        }
    }
}
