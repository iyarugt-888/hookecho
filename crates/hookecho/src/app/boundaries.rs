//! The "Forecast zones" and "CWA boundaries" reference layers: NWS forecast-zone outlines, and
//! each forecast office's county warning area, dissolved from its counties
//! ([`wxdata::ugc::cwa_outlines`]).
//!
//! Shapes come one state at a time as the view reaches them, and on native builds each state's
//! file is kept on disk (the UGC database changes a few times a year). Its own module rather than
//! more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;
use std::collections::{HashMap, HashSet};
use wxdata::overlay::{FeatureKind, GeoFeature};
use wxdata::ugc::Ugc;

/// Most states fetched for one view. Zoomed out to the whole country the layers show only the
/// states already held rather than pulling 24 MB.
const MAX_STATES_IN_VIEW: usize = 8;
/// Fetches in flight at once.
const MAX_INFLIGHT: usize = 2;
/// A cached state file older than this is fetched again.
#[cfg(not(target_arch = "wasm32"))]
const CACHE_DAYS: u64 = 30;

const ZONE_RGBA: [u8; 4] = [150, 170, 190, 150];
const CWA_RGBA: [u8; 4] = [235, 200, 120, 220];

#[derive(Default)]
pub(crate) struct BoundaryState {
    pub(crate) show_zones: bool,
    pub(crate) show_cwa: bool,
    states: HashMap<String, Vec<Ugc>>,
    inflight: HashSet<String>,
    /// States that failed, so they are not asked for every frame.
    failed: HashSet<String>,
    rx: Option<std::sync::mpsc::Receiver<(String, Option<Vec<Ugc>>)>>,
    tx: Option<std::sync::mpsc::Sender<(String, Option<Vec<Ugc>>)>>,
    /// Zone outlines and CWA boundaries of every state held, rebuilt when one arrives.
    pub(crate) zones: Vec<GeoFeature>,
    pub(crate) cwa: Vec<GeoFeature>,
}

impl BoundaryState {
    /// The reference features to draw, under everything else.
    pub(crate) fn features(&self) -> impl Iterator<Item = &GeoFeature> {
        let zones = self.show_zones.then_some(self.zones.iter());
        let cwa = self.show_cwa.then_some(self.cwa.iter());
        zones.into_iter().flatten().chain(cwa.into_iter().flatten())
    }

    fn rebuild(&mut self) {
        let mut all: Vec<&Ugc> = self.states.values().flatten().collect();
        all.sort_by(|a, b| a.code.cmp(&b.code));
        self.zones = all
            .iter()
            .filter(|u| !u.is_county())
            .map(|u| boundary(u.rings.clone(), ZONE_RGBA, &u.name, &u.code, &u.wfo))
            .collect();
        // Dissolved per state and joined: an office whose area crosses a state line gets that
        // line drawn too, which is where one state's file ends anyway.
        let counties: Vec<Ugc> = all.into_iter().filter(|u| u.is_county()).cloned().collect();
        let lines = wxdata::ugc::cwa_outlines(&counties);
        self.cwa = if lines.is_empty() {
            Vec::new()
        } else {
            vec![boundary(lines, CWA_RGBA, "County warning areas", "CWA", "")]
        };
    }
}

fn boundary(
    rings: Vec<Vec<[f64; 2]>>,
    rgba: [u8; 4],
    name: &str,
    code: &str,
    wfo: &str,
) -> GeoFeature {
    GeoFeature {
        rings,
        fill: [0, 0, 0, 0],
        stroke: rgba,
        kind: FeatureKind::Boundary,
        title: format!("{name} ({code})"),
        detail: if wfo.is_empty() {
            "NWS county warning areas, dissolved from their counties (Iowa Environmental Mesonet)"
                .to_string()
        } else {
            format!("Forecast zone {code}, {name}. Office: {wfo}.")
        },
        alert: None,
    }
}

/// Where a state's file is kept.
#[cfg(not(target_arch = "wasm32"))]
fn cache_file(state: &str) -> Option<std::path::PathBuf> {
    Some(
        crate::paths::cache_dir()?
            .join("ugc")
            .join(format!("{state}.json")),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn read_cached(state: &str) -> Option<Vec<Ugc>> {
    let path = cache_file(state)?;
    let age = std::fs::metadata(&path)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    if age > std::time::Duration::from_secs(CACHE_DAYS * 86_400) {
        return None;
    }
    wxdata::ugc::parse(&std::fs::read_to_string(path).ok()?).ok()
}

impl HookEchoApp {
    /// Bring in the states the view covers while either layer is on, and take what has arrived.
    pub(crate) fn sync_boundaries(&mut self, ctx: &egui::Context) {
        let b = &mut self.boundaries;
        let mut changed = false;
        if let Some(rx) = &b.rx {
            while let Ok((state, ugcs)) = rx.try_recv() {
                b.inflight.remove(&state);
                match ugcs {
                    Some(u) => {
                        b.states.insert(state, u);
                        changed = true;
                    }
                    None => {
                        b.failed.insert(state);
                    }
                }
            }
        }
        if changed {
            b.rebuild();
            self.rebuild_overlays();
        }
        let b = &self.boundaries;
        if !b.show_zones && !b.show_cwa {
            return;
        }
        let (w, s, e, n) = self.view_bounds();
        let wanted = wxdata::ugc::states_in_view([w, s, e, n]);
        if wanted.len() > MAX_STATES_IN_VIEW {
            return;
        }
        for state in wanted {
            let b = &mut self.boundaries;
            if b.inflight.len() >= MAX_INFLIGHT {
                break;
            }
            if b.states.contains_key(state)
                || b.inflight.contains(state)
                || b.failed.contains(state)
            {
                continue;
            }
            if b.tx.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                b.tx = Some(tx);
                b.rx = Some(rx);
            }
            let tx = b.tx.clone().expect("set above");
            b.inflight.insert(state.to_string());
            let http = self.http.clone();
            let (state, ctx) = (state.to_string(), ctx.clone());
            self.spawner.spawn(async move {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(u) = read_cached(&state) {
                    let _ = tx.send((state, Some(u)));
                    ctx.request_repaint();
                    return;
                }
                let got = match wxdata::ugc::fetch_state_text(&http, &state).await {
                    Ok(text) => match wxdata::ugc::parse(&text) {
                        Ok(u) => {
                            // The file as served goes to disk; it is what `read_cached` parses.
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                if let Some(path) = cache_file(&state) {
                                    if let Some(dir) = path.parent() {
                                        let _ = std::fs::create_dir_all(dir);
                                    }
                                    let _ = std::fs::write(path, text);
                                }
                            }
                            Some(u)
                        }
                        Err(e) => {
                            log::warn!("forecast zones for {state}: {e:#}");
                            None
                        }
                    },
                    Err(e) => {
                        log::warn!("forecast zones for {state}: {e:#}");
                        None
                    }
                };
                let _ = tx.send((state, got));
                ctx.request_repaint();
            });
        }
    }
}
