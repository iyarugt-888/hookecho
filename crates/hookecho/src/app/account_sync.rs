//! Account sync and updates: signing in, syncing settings, and checking for a new version. Moved
//! out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Ask GitHub for the newest tagged release, once per session.
    ///
    /// The list endpoint, not `/releases/latest` — see [`ui::about_window::pick_latest_tag`] for
    /// why. Thirty is more releases than this project has, and one page keeps it to one request
    /// against the 60/hour unauthenticated budget.
    pub(crate) fn check_for_update(&mut self, ctx: &egui::Context) {
        if self.update_state != ui::about_window::UpdateState::Idle {
            return;
        }
        self.update_state = ui::about_window::UpdateState::Checking;
        let http = self.http.clone();
        let tx = self.update_tx.clone();
        let ctx2 = ctx.clone();
        self.spawner.spawn(async move {
            let url = "https://api.github.com/repos/d4vid87/hookecho/releases?per_page=30";
            // GitHub rejects requests without a User-Agent.
            let body = async {
                let text = http
                    .get(url)
                    .header("User-Agent", "hookecho")
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?
                    .text()
                    .await
                    .ok()?;
                Some(text)
            }
            .await;
            // `None` means the request itself failed; a body with no version tag in it is a
            // different answer, and the two must not collapse into one message.
            let state = match body {
                Some(body) => match ui::about_window::pick_latest_tag(&body) {
                    Some(tag) => ui::about_window::compare(&tag),
                    None => ui::about_window::UpdateState::NoRelease,
                },
                None => ui::about_window::UpdateState::Failed,
            };
            let _ = tx.send(state);
            ctx2.request_repaint();
        });
    }

    /// Watch the loopback listener for the redirect, then swap the code for tokens.
    pub(crate) fn poll_login(&mut self) {
        let Some(code) = self.sync_login.as_ref().and_then(|p| p.rx.try_recv().ok()) else {
            return;
        };
        let Some(pending) = self.sync_login.take() else {
            return;
        };
        let code = match code {
            Ok(c) => c,
            Err(e) => {
                self.sync_status = format!("Sign-in failed: {e}");
                return;
            }
        };
        let (id, secret) = (
            self.settings.sync_client_id.trim().to_string(),
            self.settings.sync_client_secret.trim().to_string(),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.sync_rx = Some(rx);
        self.sync_status = "Finishing sign-in…".into();
        self.spawner.spawn(async move {
            let msg = match crate::cloud::exchange(
                &id,
                &secret,
                &code,
                &pending.verifier,
                &pending.redirect,
            )
            .await
            {
                Ok(t) => {
                    t.save();
                    SyncMsg::Signed(t)
                }
                Err(e) => SyncMsg::Error(e),
            };
            let _ = tx.send(msg);
        });
    }

    /// One sync pass: refresh the token, look at what Drive has, and push or pull accordingly.
    pub(crate) fn sync_now(&mut self) {
        let Some(tokens) = self.sync_tokens.clone() else {
            self.sync_status = "Not signed in".into();
            return;
        };
        let local = match serde_json::to_value(&self.settings) {
            Ok(v) => v,
            Err(e) => {
                self.sync_status = format!("Sync failed: {e}");
                return;
            }
        };
        let share = crate::cloud::shareable(&local);
        let hash = crate::cloud::hash(&share);
        let body = serde_json::to_string_pretty(&share).unwrap_or_default();
        let st = self.sync_state.clone();
        let (id, secret) = (
            self.settings.sync_client_id.trim().to_string(),
            self.settings.sync_client_secret.trim().to_string(),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.sync_rx = Some(rx);
        self.sync_checked = Some(wxdata::clock::Instant::now());
        self.sync_status = "Syncing…".into();
        self.spawner.spawn(async move {
            let mut tokens = tokens;
            let access = match crate::cloud::access_token(&id, &secret, &mut tokens).await {
                Ok(a) => a,
                Err(e) => {
                    let _ = tx.send(SyncMsg::Error(e));
                    return;
                }
            };
            let _ = tx.send(SyncMsg::Signed(tokens));
            let remote = match crate::cloud::fetch(&access).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(SyncMsg::Error(e));
                    return;
                }
            };
            let local_changed = hash != st.local_hash;
            let remote_changed = remote
                .as_ref()
                .is_some_and(|r| r.modified != st.remote_modified);
            let action = crate::cloud::decide(local_changed, remote_changed, remote.is_some());
            let msg = match (action, remote) {
                (crate::cloud::Action::Push, r) => {
                    match crate::cloud::push(&access, r.as_ref().map(|r| r.id.as_str()), body).await
                    {
                        Ok(modified) => SyncMsg::Pushed { modified, hash },
                        Err(e) => SyncMsg::Error(e),
                    }
                }
                (crate::cloud::Action::Pull, Some(r)) => SyncMsg::Pulled {
                    body: r.body,
                    modified: r.modified,
                },
                (crate::cloud::Action::Conflict, Some(r)) => {
                    let _ = tx.send(SyncMsg::Conflict);
                    SyncMsg::Pulled {
                        body: r.body,
                        modified: r.modified,
                    }
                }
                _ => SyncMsg::UpToDate,
            };
            let _ = tx.send(msg);
        });
    }

    /// Apply whatever the sync worker sent, and start a pass when one is due.
    pub(crate) fn poll_sync(&mut self) {
        self.poll_login();
        while let Some(msg) = self.sync_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match msg {
                SyncMsg::Signed(t) => {
                    let first = self.sync_tokens.is_none();
                    self.sync_tokens = Some(t);
                    if first {
                        self.sync_login = None;
                        self.settings.sync_enabled = true;
                        self.sync_status = "Signed in".into();
                        self.sync_now();
                    }
                }
                SyncMsg::Pulled { body, modified } => match self.apply_synced(&body) {
                    Ok(hash) => {
                        self.sync_state = crate::cloud::SyncState {
                            remote_modified: modified,
                            local_hash: hash,
                            last_sync: crate::share::now(),
                        };
                        self.sync_state.save();
                        if self.sync_status != "Kept the synced copy (both sides had edits)" {
                            self.sync_status = "Settings pulled from Drive".into();
                        }
                    }
                    Err(e) => self.sync_status = format!("Sync failed: {e}"),
                },
                SyncMsg::Pushed { modified, hash } => {
                    self.sync_state = crate::cloud::SyncState {
                        remote_modified: modified,
                        local_hash: hash,
                        last_sync: crate::share::now(),
                    };
                    self.sync_state.save();
                    self.sync_status = "Settings pushed to Drive".into();
                }
                SyncMsg::Conflict => {
                    self.sync_status = "Kept the synced copy (both sides had edits)".into();
                }
                SyncMsg::UpToDate => {
                    self.sync_state.last_sync = crate::share::now();
                    self.sync_state.save();
                    self.sync_status = "Up to date".into();
                }
                SyncMsg::Error(e) => self.sync_status = format!("Sync failed: {e}"),
            }
        }
        // Periodic pass, plus the one at startup (`sync_checked` starts unset).
        if self.settings.sync_enabled
            && self.sync_tokens.is_some()
            && self
                .sync_checked
                .is_none_or(|t| t.elapsed().as_secs() >= Self::SYNC_SECS)
        {
            self.sync_now();
        }
    }
}
