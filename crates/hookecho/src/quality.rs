//! Rendering quality profiles (ROADMAP_PARITY M5.4, 1008.md E4). Low, Balanced, High and Analysis
//! set the drawing knobs together: how finely the 3D volume is marched (on the map and in the 3D
//! Reflectivity window), whether radar gates and gridded cells are blended for display, and what
//! share of the wind particles is drawn.
//!
//! A profile is a rendering contract. It never touches a product, a tilt, a threshold, a colour
//! table, the time or anything a probe, the inspector or an export reads; Analysis turns blending
//! off so the picture shows each gate and cell as it is, which is a display choice too.

/// One of the four profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityProfile {
    Low,
    Balanced,
    High,
    Analysis,
}

/// The knobs a profile sets, as they stand. Two states with the same knobs draw alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPolicy {
    /// The map 3D's quality (`MapView::map_3d.quality_steps`, one of `view::QUALITY_PRESETS`).
    pub map3d_steps: u32,
    /// The 3D Reflectivity window's samples per pixel (`Volume3dState::steps`).
    pub window3d_steps: u32,
    /// Blend neighbouring gates and grid cells for display (`Settings::smooth_radar`).
    pub smooth: bool,
    /// Percent of the wind particles drawn (`Settings::wind_particle_pct`).
    pub particle_pct: u32,
}

impl QualityProfile {
    pub const ALL: [QualityProfile; 4] = [
        QualityProfile::Low,
        QualityProfile::Balanced,
        QualityProfile::High,
        QualityProfile::Analysis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QualityProfile::Low => "Low",
            QualityProfile::Balanced => "Balanced",
            QualityProfile::High => "High",
            QualityProfile::Analysis => "Analysis",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            QualityProfile::Low => {
                "Coarsest 3D, half the wind particles: for a slow device or a long session on battery"
            }
            QualityProfile::Balanced => "Medium 3D and three quarters of the wind particles",
            QualityProfile::High => "Finest 3D and every wind particle",
            QualityProfile::Analysis => {
                "Finest 3D, with gates and grid cells drawn as they are rather than blended"
            }
        }
    }

    pub fn policy(self) -> RenderPolicy {
        let (map3d_steps, window3d_steps, smooth, particle_pct) = match self {
            QualityProfile::Low => (64, 96, true, 50),
            QualityProfile::Balanced => (96, 160, true, 75),
            QualityProfile::High => (128, 256, true, 100),
            QualityProfile::Analysis => (128, 256, false, 100),
        };
        RenderPolicy {
            map3d_steps,
            window3d_steps,
            smooth,
            particle_pct,
        }
    }

    /// The profile whose knobs these are, or `None` when they were set one by one.
    pub fn matching(policy: &RenderPolicy) -> Option<QualityProfile> {
        QualityProfile::ALL
            .into_iter()
            .find(|q| q.policy() == *policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_uses_steps_the_3d_controls_offer() {
        for q in QualityProfile::ALL {
            let p = q.policy();
            assert!(
                crate::view::QUALITY_PRESETS
                    .iter()
                    .any(|(_, s)| *s == p.map3d_steps),
                "{q:?}: the map 3D's quality menu has {}",
                p.map3d_steps
            );
            assert!(
                crate::ui::volume3d_window::STEP_PRESETS
                    .iter()
                    .any(|(_, s)| *s == p.window3d_steps),
                "{q:?}: the 3D window's menu has {}",
                p.window3d_steps
            );
            assert!((1..=100).contains(&p.particle_pct));
            assert_eq!(QualityProfile::matching(&p), Some(q), "profiles differ");
        }
        let custom = RenderPolicy {
            particle_pct: 60,
            ..QualityProfile::High.policy()
        };
        assert_eq!(QualityProfile::matching(&custom), None);
    }

    #[test]
    fn profiles_read_back_by_name() {
        for q in QualityProfile::ALL {
            let json = serde_json::to_string(&q).unwrap();
            assert_eq!(json, format!("\"{}\"", q.label().to_lowercase()));
            assert_eq!(serde_json::from_str::<QualityProfile>(&json).unwrap(), q);
        }
    }
}
