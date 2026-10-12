//! GOES for panes whose request is not the active pane's (1010.md V15, ROADMAP_PARITY M5.1): a
//! pane in another analysis-time group (a live group beside an archive group), or one whose own
//! view picks another sector, gets its own fetch, decoded field and texture, keyed by its complete
//! request, instead of waiting for the active pane's imagery. The active pane keeps the per-layer
//! slot it always had (`goes_context::GoesSlot`, mirrored into `fields`), so every reader of the
//! active pane's GOES grid is unchanged; these slots serve the other panes' drawing
//! (`pane_gpu`'s keyed field textures), readiness and probes (`field_state_for`).
//!
//! Textures share the MRMS key namespace the renderer already draws per pane, from a range far
//! above any MRMS key, so the two caches never collide.

use super::goes_context::{GoesRequest, GoesSlot};
use super::FieldState;
use crate::render::MrmsTextureKey;
use std::collections::{HashMap, HashSet};

/// The first texture key of this cache: MRMS keys count up from 1 and never get near it.
const KEY_BASE: u64 = 1 << 48;

pub(super) struct GoesPaneSlot {
    /// The request's fetch bookkeeping: requested, accepted, last fetch.
    pub slot: GoesSlot,
    pub state: FieldState,
    pub texture: MrmsTextureKey,
}

impl GoesPaneSlot {
    /// The accepted field is this request's and within its time tolerance.
    pub fn ready(&self, request: GoesRequest) -> bool {
        self.slot
            .ready(request, self.state.grid.as_ref().map(|grid| grid.time))
    }
}

#[derive(Default)]
pub(super) struct GoesPaneCache {
    entries: HashMap<GoesRequest, GoesPaneSlot>,
    next: u64,
    dropped: Vec<MrmsTextureKey>,
}

impl GoesPaneCache {
    pub fn get(&self, request: &GoesRequest) -> Option<&GoesPaneSlot> {
        self.entries.get(request)
    }
    pub fn get_mut(&mut self, request: &GoesRequest) -> Option<&mut GoesPaneSlot> {
        self.entries.get_mut(request)
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&GoesRequest, &mut GoesPaneSlot)> {
        self.entries.iter_mut()
    }
    /// The slot for `request`, made with a texture key no other slot has had.
    pub fn ensure(&mut self, request: GoesRequest) -> &mut GoesPaneSlot {
        let next = &mut self.next;
        self.entries.entry(request).or_insert_with(|| {
            *next = next
                .checked_add(1)
                .expect("GOES texture identity exhausted");
            GoesPaneSlot {
                slot: GoesSlot::default(),
                state: FieldState::default(),
                texture: MrmsTextureKey(KEY_BASE + *next),
            }
        })
    }
    /// Keep only the requests some pane still wants; the others' textures are freed. A request
    /// comes back with a new key, so a texture is never reused for another request.
    pub fn reconcile(&mut self, wanted: &HashSet<GoesRequest>) {
        let gone: Vec<GoesRequest> = self
            .entries
            .keys()
            .filter(|r| !wanted.contains(r))
            .copied()
            .collect();
        for request in gone {
            if let Some(slot) = self.entries.remove(&request) {
                self.dropped.push(slot.texture);
            }
        }
    }
    /// Textures freed since the last call, for the renderer.
    pub fn take_dropped(&mut self) -> Vec<MrmsTextureKey> {
        std::mem::take(&mut self.dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use wxdata::goes_abi::Sector;

    fn request(at: Option<i64>) -> GoesRequest {
        GoesRequest {
            layer: Some(crate::render::FieldLayer::GoesIr),
            west: false,
            sector: Sector::Conus,
            at: at.map(|s| DateTime::<Utc>::from_timestamp(s, 0).unwrap()),
            tolerance_minutes: 10,
            recipe: None,
            blend: false,
        }
    }

    /// A live pane and an archive pane get their own slots and textures; a request no pane wants
    /// is dropped with its texture, and coming back gets a new one, never an MRMS key.
    #[test]
    fn each_request_has_its_own_slot_and_texture() {
        let mut cache = GoesPaneCache::default();
        let (live, archive) = (request(None), request(Some(1_700_000_000)));
        let a = cache.ensure(live).texture;
        let b = cache.ensure(archive).texture;
        assert_ne!(a, b);
        assert_eq!(
            cache.ensure(live).texture,
            a,
            "the same request keeps its slot"
        );
        assert!(a.0 > KEY_BASE && b.0 > KEY_BASE, "clear of MRMS keys");
        cache.reconcile(&HashSet::from([archive]));
        assert!(cache.get(&live).is_none());
        assert_eq!(cache.take_dropped(), vec![a]);
        assert!(cache.take_dropped().is_empty());
        let again = cache.ensure(live).texture;
        assert_ne!(again, a, "a returning request never reuses a freed texture");
        assert!(
            cache.get(&archive).is_some_and(|s| !s.ready(archive)),
            "not ready unfetched"
        );
    }
}
