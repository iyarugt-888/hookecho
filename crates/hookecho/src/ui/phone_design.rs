//! The phone designs: five looks for the touch chrome, chosen from Appearance.
//!
//! They differ in where the tool rail sits and what is on it, whether a colour scale and a bottom
//! tab bar are drawn, how see-through the panels are, and what accent they wear — not in what the
//! app can do. Every one drives the same registry, the same palette actions and the same state, so
//! switching design never changes a setting or loses a layer. That is why they are one table and
//! one drawing routine parameterised by it, rather than five copies of the chrome.
//!
//! The five were picked from nine mockups. Aurora and Storm are the two most different
//! *information densities* (clean, touch-first versus analyst); Carbon adds the high-contrast
//! option with labelled tools; Glass is the one that trades opacity for the map showing through and
//! adds the tab bar; Atlas 3D is the one built around the 3D modes. The four not taken —
//! Command, Analyst Pro, the second Aurora and Skyfall — are either the tablet's multi-panel
//! layout (which the desktop chrome already is), a near-duplicate of one taken, or a wallpaper.

use crate::settings::PhoneDesign;

/// Which edge of the map the tool rail hangs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A button on the tool rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailItem {
    /// Centre the map on where the device is (starting the location feed the first time).
    Locate,
    Layers,
    /// The background-map picker.
    Basemaps,
    Alerts,
    Share,
    /// The distance-measuring tool.
    Measure,
    /// Cross-section, sounding and the other analysis windows.
    Analysis,
    Settings,
}

/// A choice on the 2D / 3D / Tilt / Dual bar under the site pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The flat map, one pane.
    Flat,
    /// The volumetric 3D window.
    Volume,
    /// The map tilted toward the horizon (the map-pitch 3D view).
    Tilt,
    /// Two panes side by side.
    Dual,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Flat => "2D",
            Mode::Volume => "3D",
            Mode::Tilt => "Tilt",
            Mode::Dual => "Dual",
        }
    }
}

/// Where the colour scale goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legend {
    /// Only the thin strip along the top edge that every design keeps.
    StripOnly,
    /// A tall dBZ scale down the edge opposite the rail.
    Vertical,
    /// A small labelled scale in the map's bottom corner.
    Box,
}

/// Everything about how one design draws the phone chrome.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub rail_side: Side,
    pub rail: &'static [RailItem],
    /// Text under each rail icon.
    pub rail_labels: bool,
    pub modes: &'static [Mode],
    pub legend: Legend,
    /// A Home / Layers / Timeline / Maps / More tab bar along the bottom.
    pub bottom_nav: bool,
    /// Panel opacity, 0–255. Glass is the only one that lets the map through.
    pub panel_alpha: u8,
    /// Corner radius of panels and rail buttons, in points.
    pub corner: f32,
    /// The design's own accent, used unless the person has picked a custom one.
    pub accent: [u8; 3],
}

impl PhoneDesign {
    pub const ALL: [PhoneDesign; 5] = [
        PhoneDesign::Aurora,
        PhoneDesign::Storm,
        PhoneDesign::Carbon,
        PhoneDesign::Glass,
        PhoneDesign::Atlas,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PhoneDesign::Aurora => "Aurora",
            PhoneDesign::Storm => "Storm",
            PhoneDesign::Carbon => "Carbon",
            PhoneDesign::Glass => "Glass",
            PhoneDesign::Atlas => "Atlas 3D",
        }
    }

    pub fn tagline(self) -> &'static str {
        match self {
            PhoneDesign::Aurora => "Clean, minimal, touch-friendly",
            PhoneDesign::Storm => "Analyst-focused, data-rich",
            PhoneDesign::Carbon => "Dark, high-contrast, labelled tools",
            PhoneDesign::Glass => "Translucent, with a tab bar",
            PhoneDesign::Atlas => "Built around the 3D modes",
        }
    }

    pub fn spec(self) -> Spec {
        use RailItem as R;
        match self {
            PhoneDesign::Aurora => Spec {
                rail_side: Side::Right,
                rail: &[R::Locate, R::Layers, R::Basemaps, R::Alerts, R::Share],
                rail_labels: false,
                modes: &[Mode::Flat, Mode::Tilt],
                legend: Legend::StripOnly,
                bottom_nav: false,
                panel_alpha: 238,
                corner: 18.0,
                accent: [100, 130, 255],
            },
            PhoneDesign::Storm => Spec {
                rail_side: Side::Left,
                rail: &[R::Locate, R::Layers, R::Measure, R::Analysis, R::Alerts],
                rail_labels: false,
                modes: &[Mode::Flat, Mode::Tilt, Mode::Dual],
                legend: Legend::Vertical,
                bottom_nav: false,
                panel_alpha: 248,
                corner: 8.0,
                accent: [76, 154, 255],
            },
            PhoneDesign::Carbon => Spec {
                rail_side: Side::Left,
                rail: &[R::Locate, R::Layers, R::Basemaps, R::Analysis, R::Alerts],
                rail_labels: true,
                modes: &[Mode::Flat, Mode::Volume, Mode::Tilt],
                legend: Legend::Vertical,
                bottom_nav: false,
                panel_alpha: 252,
                corner: 12.0,
                accent: [255, 59, 48],
            },
            PhoneDesign::Glass => Spec {
                rail_side: Side::Right,
                rail: &[R::Locate, R::Layers, R::Basemaps, R::Share, R::Alerts],
                rail_labels: false,
                modes: &[Mode::Flat, Mode::Volume, Mode::Dual, Mode::Tilt],
                legend: Legend::StripOnly,
                bottom_nav: true,
                panel_alpha: 190,
                corner: 22.0,
                accent: [90, 170, 255],
            },
            PhoneDesign::Atlas => Spec {
                rail_side: Side::Right,
                rail: &[
                    R::Locate,
                    R::Layers,
                    R::Measure,
                    R::Analysis,
                    R::Basemaps,
                    R::Settings,
                ],
                rail_labels: false,
                modes: &[Mode::Flat, Mode::Volume, Mode::Tilt, Mode::Dual],
                legend: Legend::Box,
                bottom_nav: false,
                panel_alpha: 236,
                corner: 14.0,
                accent: [64, 140, 255],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_five_and_each_has_its_own_name() {
        let names: std::collections::HashSet<_> =
            PhoneDesign::ALL.iter().map(|d| d.label()).collect();
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn every_design_can_reach_the_layers_sheet_and_go_back_to_a_flat_map() {
        // Whatever the look, these two are how you get to everything else and how you get out of
        // a mode you did not mean to enter.
        for d in PhoneDesign::ALL {
            let s = d.spec();
            assert!(
                s.rail.contains(&RailItem::Layers),
                "{} has no way to Layers",
                d.label()
            );
            assert!(
                s.modes.contains(&Mode::Flat),
                "{} cannot return to 2D",
                d.label()
            );
        }
    }

    #[test]
    fn no_rail_repeats_a_button() {
        for d in PhoneDesign::ALL {
            let rail = d.spec().rail;
            for (i, a) in rail.iter().enumerate() {
                assert!(!rail[i + 1..].contains(a), "{} repeats {a:?}", d.label());
            }
        }
    }

    #[test]
    fn the_designs_differ_in_the_ways_the_mockups_do() {
        assert_eq!(PhoneDesign::Storm.spec().rail_side, Side::Left);
        assert_eq!(PhoneDesign::Carbon.spec().rail_side, Side::Left);
        assert_eq!(PhoneDesign::Aurora.spec().rail_side, Side::Right);
        assert!(PhoneDesign::Carbon.spec().rail_labels);
        assert!(!PhoneDesign::Aurora.spec().rail_labels);
        assert!(PhoneDesign::Glass.spec().bottom_nav);
        assert!(!PhoneDesign::Storm.spec().bottom_nav);
        assert!(
            PhoneDesign::Glass.spec().panel_alpha < 200,
            "Glass is the see-through one"
        );
        assert!(PhoneDesign::Storm.spec().panel_alpha > 240);
        assert_eq!(PhoneDesign::Atlas.spec().legend, Legend::Box);
        assert_eq!(PhoneDesign::Storm.spec().legend, Legend::Vertical);
    }

    #[test]
    fn only_atlas_and_glass_offer_all_four_modes_and_aurora_stays_simple() {
        assert_eq!(PhoneDesign::Atlas.spec().modes.len(), 4);
        assert_eq!(PhoneDesign::Glass.spec().modes.len(), 4);
        assert_eq!(PhoneDesign::Aurora.spec().modes.len(), 2);
    }
}
