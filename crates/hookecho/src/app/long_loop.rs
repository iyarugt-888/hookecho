//! Long loops: up to [`MAX_LOOP_FRAMES`] radar scans in one loop.
//!
//! A decoded volume is tens of megabytes, so the scan cache holds a few dozen. Beyond that a loop
//! plays **light frames**: each scan it has downloaded is also kept trimmed to the tilt and
//! moment on screen ([`wxdata::level2::trim_scan`], a few percent of the volume), and playback
//! shows those. Nothing is approximated: the light frame bins to exactly the same sweep. Pausing,
//! the 3D map, Max and Clean need the whole volume, so they load it as before.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;
use std::num::NonZeroUsize;
use std::sync::Arc;
use wxdata::level2::{Moment, Scan};

/// The longest loop the loop-length control offers.
pub(crate) const MAX_LOOP_FRAMES: usize =
    if cfg!(any(target_os = "android", target_arch = "wasm32")) {
        100
    } else {
        200
    };

/// Light frames kept: a little over the longest loop, for one moment and tilt at a time (a
/// switch of product or tilt starts filling a new set while the old one ages out).
const LIGHT_FRAMES: usize = MAX_LOOP_FRAMES + MAX_LOOP_FRAMES / 5;

type LightKey = (String, Moment, i32);

pub(crate) struct LightFrames {
    cache: lru::LruCache<LightKey, Arc<Scan>>,
}

impl Default for LightFrames {
    fn default() -> Self {
        Self {
            cache: lru::LruCache::new(NonZeroUsize::new(LIGHT_FRAMES).expect("nonzero")),
        }
    }
}

/// A tilt, to a tenth of a degree.
fn elev_key(e: f32) -> i32 {
    (e * 10.0).round() as i32
}

impl HookEchoApp {
    /// The moment and tilt pane `view` shows, as a light frame key for scan `name`.
    fn light_key(&self, view: usize, name: &str) -> Option<(LightKey, f32)> {
        let v = self.views.get(view)?;
        let elev = *v.volume.as_ref()?.elevations.get(v.tilt)?;
        Some(((name.to_string(), v.moment, elev_key(elev)), elev))
    }

    /// Keep a light copy of scan `name` for what pane `view` shows.
    pub(crate) fn remember_light(&mut self, view: usize, name: &str, scan: &Scan) {
        let Some((key, elev)) = self.light_key(view, name) else {
            return;
        };
        if self.light_frames.cache.contains(&key) {
            return;
        }
        let light = wxdata::level2::trim_scan(scan, key.1, elev);
        self.light_frames.cache.put(key, Arc::new(light));
    }

    /// The light copy of scan `name` for what pane `view` shows, if one is kept.
    pub(crate) fn light_frame(&self, view: usize, name: &str) -> Option<Arc<Scan>> {
        let (key, _) = self.light_key(view, name)?;
        self.light_frames.cache.peek(&key).cloned()
    }

    /// Whether pane `view` may show light frames now: a loop is playing on the flat map, and
    /// neither Max nor Clean (which read every tilt, or the CC) is on.
    pub(crate) fn light_ok(&self, view: usize) -> bool {
        let v = &self.views[view];
        v.timeline.playing && !v.map_3d.enabled && !v.column_max && !v.clean_reflectivity
    }
}
