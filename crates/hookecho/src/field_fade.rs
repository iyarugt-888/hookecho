//! A short visual crossfade when a field layer's frame changes (ROADMAP_PARITY M5.4, 1008.md E2).
//!
//! When a pane's model or MRMS layer moves to another texture, the renderer can keep drawing the
//! texture it showed before, under the new one, while the new one fades in over [`DURATION`].
//! Only the picture fades: probes, exports and the stamp read the new frame from its first draw.
//! A layer whose values are categories, masks, accumulations or vectors, or whose kind is not
//! known, changes instantly, so a fade never shows a colour between two classes.

use crate::render::{FieldLayer, FieldTexture};
use std::collections::HashMap;
use std::time::Duration;
use wxdata::clock::Instant;

/// How long the new frame takes to fade in.
pub const DURATION: Duration = Duration::from_millis(250);

/// One pane's fades: the texture each layer last drew, and the fades in progress.
#[derive(Debug, Default, Clone)]
pub struct FadeState {
    shown: HashMap<FieldLayer, FieldTexture>,
    fading: HashMap<FieldLayer, (FieldTexture, Instant)>,
}

impl FadeState {
    /// Whether any fade is still in progress, so the pane should repaint.
    pub fn active(&self) -> bool {
        !self.fading.is_empty()
    }
}

/// Whether `layer` may crossfade: a continuous field whose kind is known (scalar or probability,
/// the same rule as interpolated frames).
pub fn eligible(layer: FieldLayer) -> bool {
    use wxdata::field::ValueKind;
    layer
        .descriptor()
        .is_some_and(|d| matches!(d.value_kind, ValueKind::Scalar | ValueKind::Probability))
        || rtma_scalar(layer)
}

/// The RTMA observation analyses that are scalars. They carry no field descriptor, so their kind
/// is stated here. Not the hourly precipitation (an accumulation), and not the ceiling: "no
/// ceiling" is stored as a very large height, a class rather than a value, and a fade into it
/// would draw heights nobody reported.
fn rtma_scalar(layer: FieldLayer) -> bool {
    matches!(
        layer,
        FieldLayer::RtmaTemp2m
            | FieldLayer::RtmaDewpoint2m
            | FieldLayer::RtmaWind10m
            | FieldLayer::RtmaGust10m
            | FieldLayer::RtmaVisibility
            | FieldLayer::RtmaMslp
    )
}

/// Record the textures `current` this frame draws and return the fades to draw:
/// `(layer, previous texture, the new frame's share 0..1)`. A fade starts when an eligible
/// layer's texture changes while `on`; it ends after [`DURATION`], when the layer stops drawing,
/// or when `on` is cleared. A layer that stopped drawing starts over without a fade.
pub fn advance(
    state: &mut FadeState,
    current: &[(FieldLayer, FieldTexture)],
    eligible: impl Fn(FieldLayer) -> bool,
    on: bool,
    now: Instant,
) -> Vec<(FieldLayer, FieldTexture, f32)> {
    let mut shown = HashMap::with_capacity(current.len());
    for &(layer, texture) in current {
        if let Some(&before) = state.shown.get(&layer) {
            if before != texture && on && eligible(layer) {
                state.fading.insert(layer, (before, now));
            }
        }
        shown.insert(layer, texture);
    }
    state.shown = shown;
    let mut fades = Vec::new();
    state.fading.retain(|layer, (from, started)| {
        let share = now.duration_since(*started).as_secs_f32() / DURATION.as_secs_f32();
        let keep = on && share < 1.0 && state.shown.contains_key(layer);
        if keep {
            fades.push((*layer, *from, share.max(0.0)));
        }
        keep
    });
    fades
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ModelTextureKey, MrmsTextureKey};

    const REF: FieldLayer = FieldLayer::Mrms;

    fn model(k: u64) -> FieldTexture {
        FieldTexture::Model(ModelTextureKey(k))
    }

    #[test]
    fn a_new_frame_fades_in_over_the_old_one_and_then_stands_alone() {
        let mut s = FadeState::default();
        let t0 = Instant::now();
        let all = |_| true;
        assert!(advance(&mut s, &[(REF, model(1))], all, true, t0).is_empty());
        assert!(advance(&mut s, &[(REF, model(1))], all, true, t0).is_empty());
        // The frame changes: the old texture is drawn under the new one, which starts at nothing.
        let fades = advance(&mut s, &[(REF, model(2))], all, true, t0);
        assert_eq!(fades, [(REF, model(1), 0.0)]);
        let half = advance(&mut s, &[(REF, model(2))], all, true, t0 + DURATION / 2);
        assert_eq!(half.len(), 1);
        assert!((half[0].2 - 0.5).abs() < 1e-3);
        assert!(s.active());
        assert!(advance(&mut s, &[(REF, model(2))], all, true, t0 + DURATION).is_empty());
        assert!(!s.active());
    }

    #[test]
    fn categories_off_hidden_layers_and_the_setting_change_instantly() {
        let t0 = Instant::now();
        // An ineligible layer never fades.
        let mut s = FadeState::default();
        advance(&mut s, &[(REF, model(1))], |_| false, true, t0);
        assert!(advance(&mut s, &[(REF, model(2))], |_| false, true, t0).is_empty());
        // With the setting off, nothing fades.
        let mut s = FadeState::default();
        advance(&mut s, &[(REF, model(1))], |_| true, false, t0);
        assert!(advance(&mut s, &[(REF, model(2))], |_| true, false, t0).is_empty());
        // A layer that stops drawing ends its fade, and comes back without one.
        let mut s = FadeState::default();
        advance(&mut s, &[(REF, model(1))], |_| true, true, t0);
        assert_eq!(
            advance(&mut s, &[(REF, model(2))], |_| true, true, t0).len(),
            1
        );
        assert!(advance(&mut s, &[], |_| true, true, t0).is_empty());
        assert!(advance(&mut s, &[(REF, model(3))], |_| true, true, t0).is_empty());
        // Turning the setting off mid-fade ends it.
        let mut s = FadeState::default();
        advance(&mut s, &[(REF, model(1))], |_| true, true, t0);
        advance(&mut s, &[(REF, model(2))], |_| true, true, t0);
        assert!(advance(&mut s, &[(REF, model(2))], |_| true, false, t0).is_empty());
        assert!(!s.active());
    }

    #[test]
    fn only_continuous_known_fields_are_eligible() {
        assert!(eligible(FieldLayer::Mrms), "reflectivity is a scalar");
        assert!(eligible(FieldLayer::GlobalTemp2m));
        assert!(
            !eligible(FieldLayer::PrecipType),
            "precipitation type is categories"
        );
        assert!(
            !eligible(FieldLayer::ModelField),
            "a browsed field's kind is not known"
        );
        // The RTMA observation analyses: scalars fade; the accumulation and the ceiling, whose
        // "no ceiling" is a class, do not.
        assert!(eligible(FieldLayer::RtmaTemp2m));
        assert!(eligible(FieldLayer::RtmaVisibility));
        assert!(!eligible(FieldLayer::RtmaPrecip1h));
        assert!(!eligible(FieldLayer::RtmaCeiling));
        let mrms = FieldTexture::Mrms(MrmsTextureKey(1));
        assert_ne!(mrms, model(1), "the two caches are separate namespaces");
    }

    /// A per-layer (GOES) texture is told apart by its upload count: the next upload fades in.
    #[test]
    fn a_per_layer_texture_fades_when_its_upload_count_moves() {
        let ir = FieldLayer::GoesIr;
        let mut s = FadeState::default();
        let t0 = Instant::now();
        let all = |_| true;
        advance(&mut s, &[(ir, FieldTexture::Layer(ir, 1))], all, true, t0);
        assert!(advance(&mut s, &[(ir, FieldTexture::Layer(ir, 1))], all, true, t0).is_empty());
        assert_eq!(
            advance(&mut s, &[(ir, FieldTexture::Layer(ir, 2))], all, true, t0),
            [(ir, FieldTexture::Layer(ir, 1), 0.0)]
        );
    }
}
