//! Alert rules on gridded fields and ProbSevere, rotation near your places, rain arrival, and the
//! storm digest. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Run the rules on `trigger` over a freshly built lightning grid (density, or its rate of
    /// rise).
    ///
    /// The grid's own cell is the detection: a cell whose flash count clears the rule's threshold
    /// is somewhere worth knowing about. Cooldowns are per rule and place, as everywhere else.
    pub(crate) fn evaluate_grid_rules(
        &mut self,
        trigger: crate::settings::RuleTrigger,
        field: &wxdata::mrms::MrmsField,
    ) {
        use crate::rules::Detection;
        let rules: Vec<crate::settings::AlertRule> = self
            .settings
            .alert_rules
            .iter()
            .filter(|r| r.enabled && r.trigger == trigger)
            .cloned()
            .collect();
        if rules.is_empty() || field.nx == 0 || field.ny == 0 {
            return;
        }
        let (dx, dy) = (
            (field.lon_east - field.lon_west) / field.nx as f64,
            (field.lat_north - field.lat_south) / field.ny as f64,
        );
        for rule in rules {
            // The busiest qualifying cell — a rule about lightning wants the worst of it.
            let best = field
                .values
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite() && **v > 0.0)
                .map(|(i, v)| {
                    let (x, y) = (i % field.nx, i / field.nx);
                    Detection::with_strength(
                        field.lon_west + (x as f64 + 0.5) * dx,
                        field.lat_north - (y as f64 + 0.5) * dy,
                        *v as f64,
                    )
                })
                .filter(|h| crate::rules::matches(&rule, h, &self.settings))
                .max_by(|a, b| {
                    a.strength
                        .unwrap_or(0.0)
                        .total_cmp(&b.strength.unwrap_or(0.0))
                });
            if let Some(hit) = best {
                self.note_hits(&trigger, &[hit]);
                if crate::rules::compound_ok(&rule, &hit, &self.recent_for_rules()) {
                    self.fire_rule(&rule, &hit);
                }
            }
        }
    }

    /// Run the ProbSevere rules over the freshly fetched storm probabilities.
    pub(crate) fn evaluate_probsevere_rules(&mut self, feats: &[GeoFeature]) {
        use crate::rules::Detection;
        let rules: Vec<crate::settings::AlertRule> = self
            .settings
            .alert_rules
            .iter()
            .filter(|r| r.enabled && r.trigger == crate::settings::RuleTrigger::ProbSevere)
            .cloned()
            .collect();
        if rules.is_empty() {
            return;
        }
        // One detection per storm: its centroid, carrying the Severe percentage.
        let storms: Vec<Detection> = feats
            .iter()
            .filter_map(|f| {
                let pct = crate::rules::probsevere_percent(&f.detail)?;
                let ring = f.rings.first()?;
                let n = ring.len().max(1) as f64;
                let (lon, lat) = ring
                    .iter()
                    .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
                Some(Detection::with_strength(lon / n, lat / n, pct))
            })
            .collect();
        for rule in rules {
            let worst = storms
                .iter()
                .filter(|h| crate::rules::matches(&rule, h, &self.settings))
                .max_by(|a, b| {
                    a.strength
                        .unwrap_or(0.0)
                        .total_cmp(&b.strength.unwrap_or(0.0))
                })
                .copied();
            if let Some(hit) = worst {
                self.note_hits(&crate::settings::RuleTrigger::ProbSevere, &[hit]);
                if crate::rules::compound_ok(&rule, &hit, &self.recent_for_rules()) {
                    self.fire_rule(&rule, &hit);
                }
            }
        }
    }

    /// Check whether echo is heading for any watched point (saved markers + your chase position)
    /// and alert once per approach. Live data only — an ETA off an archived scan is meaningless.
    pub(crate) fn check_rain_arrival(&mut self) {
        use crate::rain_arrival::{upstream_eta, Verdict};
        if !self.settings.rain_alerts {
            self.rain_eta.clear();
            return;
        }
        let Some((dir, kt)) = self.scit_mean_motion() else {
            return;
        };
        let idx = self.active;
        if !self.views[idx].timeline.following {
            return;
        }
        // Once per volume, like compute_tds/compute_couplets: called per frame, the detector's
        // "2 consecutive scans" persistence collapses to ~33 ms and its 30-minute cooldown gets
        // re-armed by any momentary ETA gap, which is what stacks duplicate banners.
        let key = self.volume_key(idx);
        if self.rain_key.as_ref() == Some(&key) {
            return;
        }
        // Watched points: every saved marker, plus where you are if chase mode knows. Tracked by
        // id — the detector's per-place persistence must not follow a rename or a reused name.
        let mut points: Vec<(String, String, [f64; 2])> = self
            .settings
            .markers
            .iter()
            .map(|m| (m.id.clone(), m.name.clone(), [m.lon, m.lat]))
            .collect();
        if let Some((lon, lat)) = self.chase_pos {
            points.push((
                GPS_POINT_ID.to_string(),
                "your location".to_string(),
                [lon, lat],
            ));
        }
        if points.is_empty() {
            return;
        }
        let ids: Vec<String> = points.iter().map(|(id, ..)| id.clone()).collect();
        self.rain_detector.retain(&ids);

        let tilt = self.views[idx].tilt;
        let Some(sweep) = self.views[idx]
            .volume
            .as_mut()
            .and_then(|v| v.binned(Moment::Reflectivity, tilt, false).ok())
            .cloned()
        else {
            return;
        };
        // Only now — a failed decode should retry next frame, not skip the volume.
        self.rain_key = Some(key);
        let sample = refl_sampler(&sweep);

        let mut fired = false;
        self.rain_eta.clear();
        for (id, name, at) in &points {
            let eta = upstream_eta(
                &sample,
                *at,
                dir as f64,
                kt as f64,
                crate::rain_arrival::MAX_MIN,
            );
            if let Some(min) = eta {
                self.rain_eta.push((name.clone(), min));
            }
            if let Verdict::Fire(min) = self.rain_detector.update(id, eta) {
                self.notify_alert(
                    &format!("\u{1f327} Rain reaching {name}"),
                    &format!("About {min:.0} minutes out"),
                    false,
                );
                self.banner(
                    format!("\u{1f327} Rain reaching {name}"),
                    format!("~{min:.0} min"),
                );
                fired = true;
            }
        }
        if fired && self.settings.alert_sound {
            self.play_alert(&self.settings.rain_sound.clone());
        }
    }

    /// "There is rotation near a place you care about" — the detection above fires once for the
    /// whole radar, which tells you a couplet exists somewhere in a 150 km circle. This one names
    /// the place and the distance, and it re-fires as a storm works down a line, so it is the
    /// alert worth pushing to a phone.
    ///
    /// Same shape as the lightning alarm: a per-location cooldown, so a couplet that persists over
    /// six volumes is one alert, not six.
    pub(crate) fn rotation_near_you(&mut self, hits: &[wxdata::rotation::CoupletHit]) {
        let metric = self.metric();
        const COOLDOWN: std::time::Duration = std::time::Duration::from_secs(600);
        if hits.is_empty() {
            return;
        }
        // Strongest couplet within each watched radius, if any.
        let near: Vec<(String, String, f64, f32)> = self
            .watched_points()
            .into_iter()
            .filter_map(|p| {
                let radius_km = p.radius_mi * crate::geo::KM_PER_MILE;
                hits.iter()
                    .filter_map(|h| {
                        let (km, _) = crate::geo::great_circle([p.lon, p.lat], [h.lon, h.lat]);
                        (km <= radius_km).then_some((km, h.vrot_ms))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(km, vrot)| (p.id, p.name, km, vrot))
            })
            .collect();
        let mut fired = false;
        for (id, name, km, vrot_ms) in near {
            if self
                .rotation_alerted
                .get(&id)
                .is_some_and(|t| t.elapsed() < COOLDOWN)
            {
                continue;
            }
            self.rotation_alerted.insert(id, Instant::now());
            let kt = vrot_ms as f64 * 1.943_844;
            let away = crate::geo::fmt_distance(km, metric, 0);
            self.banner(
                format!("\u{21bb} Rotation near {name}"),
                format!("{kt:.0} kt couplet, {away} away"),
            );
            self.notify_alert(
                &format!("\u{21bb} Rotation near {name}"),
                &format!("{kt:.0} kt rotational velocity, {away} from {name}"),
                true,
            );
            fired = true;
        }
        if fired && self.settings.alert_sound {
            self.play_alert_urgent(&self.settings.rotation_sound.clone());
        }
    }

    /// Build a plain-language briefing of the in-view weather. The templated summary shows
    /// instantly; if the chosen AI provider has a key, its model rewrites it in the background.
    pub(crate) fn generate_digest(&mut self, ctx: &egui::Context) {
        let brief = self.storm_brief(ctx);
        let facts = brief.fact_sheet();
        log::info!(target: "hookecho::digest", "storm brief:
{facts}");
        self.digest_window.text = brief.summary();
        self.digest_window.facts = facts.clone();
        self.digest_window.enhanced = false;
        self.digest_window.error = None;

        // Optional enhancement by the chosen model (Settings > General > AI).
        let provider = self.settings.ai_provider;
        let key = match provider {
            crate::digest::Provider::Anthropic => &self.settings.anthropic_key,
            crate::digest::Provider::Gemini => &self.settings.gemini_key,
        }
        .trim()
        .to_string();
        if key.is_empty() {
            return;
        }
        self.digest_window.provider = provider.model_name();
        let (tx, rx) = std::sync::mpsc::channel();
        self.digest_rx = Some(rx);
        self.digest_window.busy = true;
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = crate::digest::enhance(&http, provider, &key, &facts)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }
}
