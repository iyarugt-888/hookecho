//! Rebuilding derived products and the tessellated overlay when their inputs change.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Recompute the locally derived products (VIL, VIL density, echo tops) when the active pane's
    /// volume, the echo-top threshold, or the set of enabled derived layers changed.
    ///
    /// Unlike every other field layer this costs no network — the volume is already decoded here —
    /// so it has no cadence: it recomputes exactly when its inputs move, which is what makes it
    /// work in archive replay and on each live tilt.
    pub(crate) fn recompute_derived(&mut self, ctx: &egui::Context) {
        use crate::render::FieldLayer as FL;
        use radar_products::{HAIL_BITS, LAYERS};
        let Some(key) = self.current_derived_key() else {
            self.derived_key = None;
            return;
        };
        let mask = key.layers;
        // The hail algorithm needs the melting level: the live HRRR analysis while following the
        // feed, the observed sounding from that day on an archived volume (`freezing_for` never
        // mixes the two). Only worth a request when a hail grid is actually on.
        let levels = if mask & HAIL_BITS != 0 {
            self.freezing_for(self.active)
        } else {
            None
        };
        if mask & HAIL_BITS != 0 && levels.is_none() {
            self.fetch_freezing_levels(ctx, self.active);
        }
        // Beam heights are above the radar; the model heights are above sea level.
        let radar_m = key
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map_or(0.0, |s| s.elevation_meters as f64);
        if self.derived_key.as_ref() == Some(&key) {
            return;
        }
        let Some(vol) = self.views[self.active].volume.as_mut() else {
            return;
        };
        // Binning is cached on the volume; the integral is the expensive half and runs off-thread.
        let mut sweeps = vol.reflectivity_tilts();
        if sweeps.len() < 2 {
            return;
        }
        let opts = wxdata::derived::DerivedOpts {
            etop_dbz: self.settings.etop_dbz,
            time: vol.time,
            ..Default::default()
        };
        self.derived_key = Some(key.clone());
        let tx = self.overlay_tx.clone();
        let lane = RequestLane::Feed(FeedSource::DerivedRadarFields);
        let generation = self.acquisition.start(lane.clone());
        let cap = self.field_texture_cap();
        let ctx = ctx.clone();
        self.spawner.spawn_blocking(move || {
            let result = (|| -> anyhow::Result<_> {
                let coverage = wxdata::level2::temporal::prepare_with_passes(
                    &mut sweeps,
                    key.policy,
                    key.pass_index(),
                )?;
                let mut out: Vec<(FL, wxdata::mrms::MrmsField)> = Vec::new();
                if mask & !HAIL_BITS != 0 {
                    if let Some(d) = wxdata::derived::derive(&sweeps, &opts) {
                        out.extend([
                            (FL::CompositeLocal, d.composite),
                            (FL::VilLocal, d.vil),
                            (FL::VilDensity, d.vild),
                            (FL::EtopLocal, d.etop),
                        ]);
                    }
                }
                if let Some((h0, hm20)) = levels.filter(|_| mask & HAIL_BITS != 0) {
                    if let Some(h) =
                        wxdata::derived::hail(&sweeps, h0 - radar_m, hm20 - radar_m, &opts)
                    {
                        out.extend([(FL::HailMehs, h.mehs), (FL::HailPosh, h.posh)]);
                    }
                }
                out.retain(|(layer, _)| {
                    let bit = LAYERS.iter().position(|l| l == layer).unwrap_or(0);
                    mask & (1 << bit) != 0
                });
                anyhow::ensure!(
                    !out.is_empty(),
                    "no local radar fields available for this scan and environment"
                );
                Ok((
                    coverage,
                    out.into_iter()
                        .map(|(layer, f)| (layer, f.decimated(cap)))
                        .collect(),
                ))
            })()
            .map_err(|error| error.to_string());
            let _ = tx.send(OverlayDelivery::Fetched {
                lane,
                generation,
                result: Ok(OverlayMsg::DerivedFields(Box::new(
                    radar_products::DerivedDelivery {
                        key,
                        fields: result,
                    },
                ))),
            });
            ctx.request_repaint();
        });
    }

    /// Reassemble the displayed overlay set from the fetched sources and current filters.
    pub(crate) fn rebuild_overlays(&mut self) {
        // Reference lines first, so every product draws over them.
        let mut v: Vec<GeoFeature> = self.boundaries.features().cloned().collect();
        if (1..=8).contains(&self.filters.outlook_day) {
            v.extend(
                self.outlook_features[(self.filters.outlook_day - 1) as usize]
                    .iter()
                    .cloned(),
            );
        }
        if self.filters.show_mds {
            // The discussions in effect at a scrubbed frame's time, or today's.
            let archived = self.arch_md_shown.and_then(|b| self.arch_mds.peek(&b));
            v.extend(archived.unwrap_or(&self.md_features).iter().cloned());
        }
        if self.filters.show_watches {
            v.extend(self.watch_features.iter().cloned());
        }
        if (1..=3).contains(&self.filters.wssi_day) {
            v.extend(self.wssi_features.iter().cloned());
        }
        if (1..=3).contains(&self.filters.ero_day) {
            v.extend(self.ero_features.iter().cloned());
        }
        if (1..=2).contains(&self.filters.fire_day) {
            v.extend(self.fire_features.iter().cloned());
        }
        if self.show_outages {
            v.extend(self.outage_features.iter().cloned());
        }
        if self.filters.show_alerts {
            for f in self.active_alert_features() {
                if self.filters.alert_cats[alerts::category(&f.title).index()] {
                    v.push(f.clone());
                }
            }
        }
        if self.show_probsevere {
            v.extend(self.probsevere.iter().cloned());
        }
        if self.show_tropical {
            if let Some(t) = &self.tropical {
                // Surge and wind field go under the cones: the cone is the headline, these are
                // the context it sits on.
                v.extend(t.surge.iter().cloned());
                v.extend(t.wind_radii.iter().cloned());
                v.extend(t.cones.iter().cloned());
            }
        }
        if self.show_aviation {
            v.extend(self.aviation_features.iter().cloned());
        }
        if self.show_tfr {
            // Shapes that failed to parse are kept as empty placeholders so they are not
            // refetched forever; they have nothing to draw.
            v.extend(
                self.tfr_features
                    .values()
                    .filter(|f| !f.rings.is_empty())
                    .cloned(),
            );
        }
        if self.show_fires {
            v.extend(self.fire_perims.iter().cloned());
        }
        if self.show_imported_gis {
            let style = self.settings.imported_gis_style;
            self.refresh_imported_colors();
            let colors = self.imported_colors.as_ref().map(|(_, c, _)| c);
            let src = &self.imported_marks.shape_src;
            let shown = |i: usize| {
                self.imported_shown
                    .as_ref()
                    .is_none_or(|m| src.get(i).and_then(|&s| m.get(s)).copied().unwrap_or(true))
            };
            let imported = self
                .imported_gis
                .iter()
                .enumerate()
                .filter(|&(i, _)| shown(i))
                .map(|(i, feature)| {
                    let mut feature = feature.clone();
                    let mut style = style;
                    if let Some(c) = colors.and_then(|c| *c.get(*src.get(i)?)?) {
                        style.color = c;
                    }
                    crate::gis_import::apply_style(&mut feature, style);
                    feature
                });
            // The list is painted in order: first is underneath (I4 z-order).
            if self.settings.imported_gis_below {
                v.splice(0..0, imported);
            } else {
                v.extend(imported);
            }
        }
        self.overlays = v;
        self.overlay_gen = self.overlay_gen.wrapping_add(1);
    }
}
