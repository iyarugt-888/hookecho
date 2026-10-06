//! Shared display slots by complete MRMS request, with bounded off-screen retention.
use super::{FieldState, MrmsContext};
use crate::render::MrmsTextureKey;
use lru::LruCache;
use std::{collections::HashSet, num::NonZeroUsize, time::Duration};
use wxdata::clock::Instant;

pub(super) struct MrmsSlot {
    pub state: FieldState,
    pub texture: MrmsTextureKey,
    pub generation: u64,
    pub precip: Option<std::sync::Arc<super::PrecipGrid>>,
}

impl MrmsSlot {
    pub fn ready(&self, request: MrmsContext) -> bool {
        self.state.mrms_ready(&request.request()) && self.state.grid.is_some()
    }
    pub fn due(&self, request: MrmsContext, cadence: Duration, now: Instant) -> bool {
        !(request.archive.is_some() && self.ready(request))
            && self.state.mrms_due(&request.request(), true, cadence, now)
    }
    pub fn stage(
        &mut self,
        request: MrmsContext,
        field: wxdata::field::Stamped<wxdata::mrms::MrmsField>,
        upload: crate::render::MrmsUpload,
        now: Instant,
    ) -> bool {
        if !request.accepts_field(&field) {
            return false;
        }
        if request.layer == crate::render::FieldLayer::PrecipType {
            self.precip = Some(std::sync::Arc::new(super::PrecipGrid::new(&field.data)));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("MRMS field generation exhausted");
        self.state.stage(field.data, Some(field.stamp), upload);
        self.state.mrms_delivered(request.request(), now);
        true
    }
}

pub(super) struct MrmsFieldCache {
    entries: LruCache<MrmsContext, MrmsSlot>,
    next: u64,
    resting_capacity: usize,
    dropped: Vec<(MrmsContext, MrmsTextureKey)>,
}
impl Default for MrmsFieldCache {
    fn default() -> Self {
        Self::new(if cfg!(target_os = "android") { 12 } else { 32 })
    }
}
impl MrmsFieldCache {
    pub fn new(resting_capacity: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(resting_capacity.max(1)).unwrap()),
            next: 0,
            resting_capacity: resting_capacity.max(1),
            dropped: Vec::new(),
        }
    }
    pub fn get(&self, request: &MrmsContext) -> Option<&MrmsSlot> {
        self.entries.peek(request)
    }
    pub fn get_mut(&mut self, request: &MrmsContext) -> Option<&mut MrmsSlot> {
        self.entries.peek_mut(request)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&MrmsContext, &MrmsSlot)> {
        self.entries.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&MrmsContext, &mut MrmsSlot)> {
        self.entries.iter_mut()
    }
    /// Promote all visible contexts before eviction, so no pane can evict another pane's field.
    pub fn reconcile(&mut self, wanted: &HashSet<MrmsContext>, now: Instant) {
        for request in wanted {
            if let Some(slot) = self.entries.get_mut(request) {
                slot.state.off_since = None;
            }
        }
        let expired: Vec<_> = self
            .entries
            .iter_mut()
            .filter_map(|(request, slot)| {
                if wanted.contains(request) {
                    return None;
                }
                match slot.state.off_since {
                    Some(since)
                        if now.saturating_duration_since(since) >= Duration::from_secs(60) =>
                    {
                        Some(*request)
                    }
                    Some(_) => None,
                    None => {
                        slot.state.off_since = Some(now);
                        None
                    }
                }
            })
            .collect();
        for request in expired {
            if let Some(slot) = self.entries.pop(&request) {
                self.dropped.push((request, slot.texture));
            }
        }
        let capacity = self.resting_capacity.max(wanted.len()).max(1);
        while self.entries.len() > capacity {
            if let Some((request, slot)) = self.entries.pop_lru() {
                self.dropped.push((request, slot.texture));
            }
        }
        self.entries.resize(NonZeroUsize::new(capacity).unwrap());
    }
    pub fn ensure(&mut self, request: MrmsContext) -> &mut MrmsSlot {
        if !self.entries.contains(&request) {
            self.next = self
                .next
                .checked_add(1)
                .expect("MRMS texture identity exhausted");
            let slot = MrmsSlot {
                state: FieldState::default(),
                texture: MrmsTextureKey(self.next),
                generation: 0,
                precip: None,
            };
            if let Some((old_request, old)) = self.entries.push(request, slot) {
                self.dropped.push((old_request, old.texture));
            }
        }
        self.entries.get_mut(&request).unwrap()
    }
    pub fn take_dropped(&mut self) -> Vec<(MrmsContext, MrmsTextureKey)> {
        std::mem::take(&mut self.dropped)
    }
}
