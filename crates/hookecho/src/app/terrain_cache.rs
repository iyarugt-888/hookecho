//! Ground elevation for the app (ROADMAP_PARITY M3.6): terrain tiles ([`wxdata::terrain`])
//! fetched when a tool asks for the ground under a point, kept for the session. A read never
//! waits: it answers with the height when the tile is here, says it is coming while the fetch
//! runs, and says it is unknown when the tile cannot be had — never a guessed ground.
use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use wxdata::terrain::{Tile, TileId};

/// The ground under a point, as far as is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Ground {
    /// Metres above mean sea level, and the grid's resolution there in metres.
    Known {
        msl_m: f64,
        resolution_m: f64,
    },
    Loading,
    Unknown,
}

enum TileState {
    Pending,
    Ready(Arc<Tile>),
    Failed,
}

type Arrival = (TileId, Option<Tile>);

#[derive(Default)]
pub(crate) struct TerrainCache {
    tiles: HashMap<TileId, TileState>,
    rx: Option<std::sync::mpsc::Receiver<Arrival>>,
    tx: Option<std::sync::mpsc::Sender<Arrival>>,
}

/// Most tiles kept (about 256 KB of heights each).
const MAX_TILES: usize = 256;

impl TerrainCache {
    /// The ground under `(lon, lat)` from the tiles already here.
    pub(crate) fn ground(&self, lon: f64, lat: f64) -> Ground {
        let Some((id, px, py)) = wxdata::terrain::locate(lon, lat, wxdata::terrain::ZOOM) else {
            return Ground::Unknown;
        };
        match self.tiles.get(&id) {
            Some(TileState::Ready(t)) => Ground::Known {
                msl_m: f64::from(t.sample(px, py)),
                resolution_m: wxdata::terrain::resolution_m(lat, id.z),
            },
            Some(TileState::Pending) => Ground::Loading,
            Some(TileState::Failed) => Ground::Unknown,
            None => Ground::Loading,
        }
    }

    /// Take finished fetches; once a frame.
    pub(crate) fn take_arrivals(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok((id, tile)) = rx.try_recv() {
                let state = tile.map_or(TileState::Failed, |t| TileState::Ready(Arc::new(t)));
                self.tiles.insert(id, state);
            }
        }
    }
}

impl HookEchoApp {
    /// Start fetching the tile under `(lon, lat)` unless it is here or on its way.
    pub(crate) fn request_ground(&mut self, lon: f64, lat: f64, ctx: &egui::Context) {
        let Some((id, _, _)) = wxdata::terrain::locate(lon, lat, wxdata::terrain::ZOOM) else {
            return;
        };
        let cache = &mut self.terrain;
        if cache.tiles.contains_key(&id) {
            return;
        }
        if cache.tiles.len() >= MAX_TILES {
            // A session that has looked at this much ground starts the cache over.
            cache.tiles.retain(|_, s| matches!(s, TileState::Pending));
        }
        if cache.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            cache.tx = Some(tx);
            cache.rx = Some(rx);
        }
        let tx = cache.tx.clone().expect("set above");
        cache.tiles.insert(id, TileState::Pending);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let tile = match wxdata::terrain::fetch(&http, id).await {
                Ok(t) => Some(t),
                Err(e) => {
                    log::warn!("terrain tile {id:?}: {e:#}");
                    None
                }
            };
            let _ = tx.send((id, tile));
            ctx.request_repaint();
        });
    }
}
