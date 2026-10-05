//! Shared display slots by complete model request, with bounded off-screen retention.
use super::{FieldState, ModelRequest};
use crate::render::ModelTextureKey;
use lru::LruCache;
use std::{collections::HashSet, num::NonZeroUsize, time::Duration};
use wxdata::clock::Instant;

pub(super) struct ModelSlot {
    pub state: FieldState,
    pub texture: ModelTextureKey,
}

pub(super) struct ModelFieldCache {
    entries: LruCache<ModelRequest, ModelSlot>,
    next: u64,
    resting_capacity: usize,
    dropped: Vec<(ModelRequest, ModelTextureKey)>,
}
impl Default for ModelFieldCache {
    fn default() -> Self {
        Self::new(if cfg!(target_os = "android") { 12 } else { 32 })
    }
}
impl ModelFieldCache {
    pub fn new(resting_capacity: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(resting_capacity.max(1)).unwrap()),
            next: 0,
            resting_capacity: resting_capacity.max(1),
            dropped: Vec::new(),
        }
    }
    pub fn get(&self, request: &ModelRequest) -> Option<&ModelSlot> {
        self.entries.peek(request)
    }
    pub fn get_mut(&mut self, request: &ModelRequest) -> Option<&mut ModelSlot> {
        self.entries.peek_mut(request)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ModelRequest, &ModelSlot)> {
        self.entries.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ModelRequest, &mut ModelSlot)> {
        self.entries.iter_mut()
    }
    /// Promote all visible contexts before eviction, so no pane can evict another pane's field.
    pub fn reconcile(&mut self, wanted: &HashSet<ModelRequest>, now: Instant) {
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
    pub fn ensure(&mut self, request: ModelRequest) -> &mut ModelSlot {
        if !self.entries.contains(&request) {
            self.next = self
                .next
                .checked_add(1)
                .expect("Model texture identity exhausted");
            let slot = ModelSlot {
                state: FieldState::default(),
                texture: ModelTextureKey(self.next),
            };
            if let Some((old_request, old)) = self.entries.push(request, slot) {
                self.dropped.push((old_request, old.texture));
            }
        }
        self.entries.get_mut(&request).unwrap()
    }
    pub fn take_dropped(&mut self) -> Vec<(ModelRequest, ModelTextureKey)> {
        std::mem::take(&mut self.dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::FieldLayer;
    use wxdata::global::{GlobalField, GlobalModel};

    fn request(hour: u16) -> ModelRequest {
        ModelRequest::Global(
            FieldLayer::GlobalTemp2m,
            GlobalModel::Gfs,
            GlobalField::Temp2m,
            hour,
            None,
        )
    }

    #[test]
    fn model_cache_identical_requests_share_storage_and_different_runs_do_not() {
        let mut cache = ModelFieldCache::new(3);
        let a = request(6);
        let b = ModelRequest::Global(
            FieldLayer::GlobalTemp2m,
            GlobalModel::Gfs,
            GlobalField::Temp2m,
            6,
            chrono::DateTime::from_timestamp(1_700_000_000, 0),
        );
        let key = cache.ensure(a).texture;
        assert_eq!(cache.ensure(a).texture, key);
        assert_ne!(cache.ensure(b).texture, key);
        assert_eq!(cache.iter().count(), 2);
    }

    #[test]
    fn model_cache_all_visible_panes_survive_growth_and_shrink() {
        let mut cache = ModelFieldCache::new(2);
        let now = Instant::now();
        for hour in 0..2 {
            cache.ensure(request(hour));
        }
        let visible: HashSet<_> = (5..17).map(request).collect();
        cache.reconcile(&visible, now);
        for req in &visible {
            cache.ensure(*req);
        }
        assert_eq!(cache.iter().count(), 12);
        assert!(visible.iter().all(|req| cache.get(req).is_some()));
        let remaining = HashSet::from([request(5), request(16)]);
        cache.reconcile(&remaining, now);
        assert_eq!(cache.iter().count(), 2);
        assert!(remaining.iter().all(|req| cache.get(req).is_some()));
        assert_eq!(cache.take_dropped().len(), 12);
    }

    #[test]
    fn model_cache_hidden_slots_expire_and_texture_ids_are_never_reused() {
        let mut cache = ModelFieldCache::new(2);
        let now = Instant::now();
        let old = cache.ensure(request(1)).texture;
        cache.reconcile(&HashSet::new(), now);
        cache.reconcile(&HashSet::new(), now + Duration::from_secs(59));
        assert!(cache.get(&request(1)).is_some());
        cache.reconcile(&HashSet::new(), now + Duration::from_secs(60));
        assert_eq!(cache.take_dropped(), [(request(1), old)]);
        assert_ne!(cache.ensure(request(1)).texture, old);
    }

    #[test]
    fn model_cache_visible_slot_cannot_expire_from_an_old_hidden_clock() {
        let mut cache = ModelFieldCache::new(1);
        let now = Instant::now();
        let old = cache.ensure(request(1)).texture;
        cache.reconcile(&HashSet::new(), now);
        cache.reconcile(&HashSet::from([request(1)]), now + Duration::from_secs(100));
        assert_eq!(cache.get(&request(1)).unwrap().texture, old);
        assert!(cache.take_dropped().is_empty());
    }
}
