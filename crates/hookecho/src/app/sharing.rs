//! Sharing: share cards and links, snapshots pushed and screenshots saved. Moved out of
//! `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Push our position out and pull everyone else's in. Sharing runs whenever the setting is on
    /// — receiving works even with no fix of our own, which is the desktop-at-home half of it.
    pub(crate) fn sync_share(&mut self, ctx: &egui::Context) {
        if !self.settings.share_position {
            if self.share.is_some() {
                self.share = None;
                self.peers.clear();
            }
            return;
        }
        let share = self.share.get_or_insert_with(crate::share::Share::start);
        if share.drain(&mut self.peers) {
            ctx.request_repaint();
        }
        let due = self
            .share_sent
            .is_none_or(|t| t.elapsed().as_secs() >= Self::SHARE_SECS);
        let Some((lon, lat)) = self.chase_pos.filter(|_| due) else {
            return;
        };
        self.share_sent = Some(wxdata::clock::Instant::now());
        let name = if self.settings.share_name.is_empty() {
            "me"
        } else {
            &self.settings.share_name
        };
        let me = share.me(name, lon, lat, &self.settings.share_video_url);
        share.broadcast(&me);
        let relay = self.settings.share_relay.clone();
        if relay.is_empty() {
            return;
        }
        // Relay round trip: hand it our fix, take back the list. One request pair per tick, and
        // failures are logged rather than surfaced — a dead relay must not break chase mode.
        let tx = share.sender();
        let id = share.id.clone();
        let client = self.http.clone();
        self.spawner.spawn(async move {
            let timeout = std::time::Duration::from_secs(Self::SHARE_SECS - 2);
            let body = match serde_json::to_string(&me) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("share encode failed: {e}");
                    return;
                }
            };
            if let Err(e) = client
                .post(&relay)
                .header("content-type", "application/json")
                .body(body)
                .timeout(timeout)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
            {
                log::warn!("share relay post failed: {e}");
                return;
            }
            let response = match client
                .get(&relay)
                .timeout(timeout)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
            {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("share relay get failed: {e}");
                    return;
                }
            };
            match response
                .text()
                .await
                .map_err(|e| e.to_string())
                .and_then(|t| {
                    serde_json::from_str::<Vec<crate::share::Peer>>(&t).map_err(|e| e.to_string())
                }) {
                Ok(list) => {
                    for p in list.into_iter().filter(|p| p.id != id) {
                        let _ = tx.send(p);
                    }
                }
                Err(e) => log::warn!("share relay list unreadable: {e}"),
            }
        });
    }

    /// PUT a captured frame to the user's ntfy topic as an attachment.
    pub(crate) fn push_snapshot(&self, title: String, image: &egui::ColorImage) {
        let topic = self.settings.ntfy_topic.trim().to_string();
        if topic.is_empty() {
            return;
        }
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for px in &image.pixels {
            rgba.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        if let Err(e) = image::write_buffer_with_format(
            &mut png,
            &rgba,
            w,
            h,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        ) {
            log::warn!("alert snapshot encode failed: {e}");
            return;
        }
        let body = png.into_inner();
        // ntfy caps attachments (a few MB on the public server); a screen-sized PNG is well under,
        // but say so in the log rather than wondering why nothing arrived.
        log::debug!("pushing alert snapshot: {} KiB", body.len() / 1024);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = http
                .put(format!("https://ntfy.sh/{topic}"))
                .header("Title", title)
                .header("Filename", "radar.png")
                .body(body)
                .send()
                .await;
            if let Err(e) = res {
                log::warn!("ntfy snapshot push failed: {e}");
            }
        });
    }

    /// The share card: a caption band across the bottom of the map naming what the picture shows,
    /// drawn only on the frames a capture is waiting for. A radar screenshot with no site, product
    /// or time on it is a pretty picture; this is the difference between that and a report.
    pub(crate) fn share_card_footer(&mut self, ctx: &egui::Context) {
        let Some((_, frames)) = &mut self.share_card else {
            return;
        };
        *frames -= 1;
        let fire = *frames == 0;

        let v = &self.views[self.active];
        let site = v.site.clone().unwrap_or_else(|| "no site".to_string());
        let product = crate::products::info(v.moment).name.to_string();
        let time = v
            .volume
            .as_ref()
            .map(|vol| crate::timefmt::fmt_date_clock(vol.time, self.active_tz()))
            .unwrap_or_default();
        let accent = crate::theme::accent(self.settings.theme);
        let logo = crate::icon::texture(ctx, 64);
        egui::Area::new(egui::Id::new("share_card"))
            .order(egui::Order::Foreground)
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -10.0))
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, 245).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&site)
                                .size(crate::ui::style::FONT_BASE)
                                .strong()
                                .color(accent),
                        );
                        let mut line = product;
                        if !time.is_empty() {
                            line.push_str(" · ");
                            line.push_str(&time);
                        }
                        ui.label(
                            egui::RichText::new(line)
                                .size(crate::ui::style::FONT_BASE)
                                .color(egui::Color32::from_gray(238)),
                        );
                        // The mark rides with the caption: a shared loop travels without the app
                        // around it, and the wordmark alone is not what anyone recognises.
                        ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(16.0, 16.0)));
                        ui.label(
                            egui::RichText::new("HookEcho · data: NOAA/NWS")
                                .size(crate::ui::style::FONT_SM)
                                .color(egui::Color32::from_gray(160)),
                        );
                    });
                });
            });

        if fire {
            // The band is on screen: take the picture, and keep the caption for this one frame.
            let (dest, _) = self.share_card.take().expect("checked above");
            self.screenshot_pending = Some(dest);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
    }

    /// If a screenshot was requested, save the delivered image event to the pending path.
    pub(crate) fn save_pending_screenshot(&mut self, ctx: &egui::Context) {
        if self.screenshot_pending.is_none() {
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let dest = self.screenshot_pending.take().unwrap();
            match dest {
                ShotDest::File(path) => {
                    let (w, h) = (image.size[0] as u32, image.size[1] as u32);
                    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                    for px in &image.pixels {
                        rgba.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
                    }
                    match image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8) {
                        Ok(()) => {
                            log::info!("screenshot saved: {}", path.display());
                            let msg = format!("Saved {}", path.display());
                            self.toast(ToastKind::Success, msg);
                        }
                        Err(e) => {
                            log::warn!("screenshot save failed: {e}");
                            self.toast(ToastKind::Error, format!("Screenshot failed: {e}"));
                        }
                    }
                }
                ShotDest::Clipboard => {
                    // `image` here is already an egui ColorImage; hand it straight to the clipboard.
                    ctx.copy_image((*image).clone());
                    log::info!("view copied to clipboard");
                    self.toast(ToastKind::Success, "View copied to clipboard");
                }
                ShotDest::Loop => self.record_loop_frame(&image),
                ShotDest::Push(title) => self.push_snapshot(title, &image),
                ShotDest::Widget(path) => self.save_widget_snapshot(&path, &image),
                ShotDest::Report => self.write_report(&image),
                ShotDest::SceneThumb(name) => self.store_scene_thumbnail(ctx, &name, &image),
                #[cfg(not(target_arch = "wasm32"))]
                ShotDest::Api(reply) => Self::answer_api_snapshot(&reply, &image),
            }
        }
    }
}
