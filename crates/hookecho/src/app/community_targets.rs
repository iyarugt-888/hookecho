//! Towns in a storm's path (ROADMAP_PARITY M2.3): on request, the Census 2020 places touching a
//! storm's projected swath, with whole-place populations and each place's internal point, from
//! the same TIGERweb service the alert cards use ([`wxdata::census::places_in`]). Each becomes a
//! point target for the storm's arrivals, labelled as its centre point, never as its area: the
//! service gives the point, and a place's edge can be reached well before it.
//!
//! Looked up once per swath (its geometry is the key) and kept for the session; a lookup is the
//! analyst's choice, since it leaves the machine.
use super::*;
use std::collections::HashMap;
use wxdata::census::Place;

/// Most places one lookup returns.
pub(crate) const MAX_TOWNS: usize = 25;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TownsState {
    Pending,
    Ready(Vec<Place>),
    Failed(String),
}

type Arrival = (String, Result<Vec<Place>, String>);

#[derive(Default)]
pub(crate) struct TownsBook {
    by_key: HashMap<String, TownsState>,
    rx: Option<std::sync::mpsc::Receiver<Arrival>>,
    tx: Option<std::sync::mpsc::Sender<Arrival>>,
}

impl TownsBook {
    pub(crate) fn state(&self, key: &str) -> Option<&TownsState> {
        self.by_key.get(key)
    }

    fn take_arrivals(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok((key, got)) = rx.try_recv() {
                let state = match got {
                    Ok(p) => TownsState::Ready(p),
                    Err(e) => TownsState::Failed(e),
                };
                self.by_key.insert(key, state);
            }
        }
    }
}

/// A place as a storm target: its name without the Census's kind suffix, its 2020 population,
/// and that the point is its centre.
pub(crate) fn place_target(p: &Place) -> crate::app::gis_layers::Target {
    let name = ["city", "town", "village", "borough", "CDP"]
        .iter()
        .find_map(|k| p.name.strip_suffix(&format!(" {k}")))
        .unwrap_or(&p.name);
    crate::app::gis_layers::Target {
        name: format!("{name}, pop. {}", super::impact::thousands(p.population)),
        layer: "Census 2020, town centre".into(),
        shape: crate::app::gis_layers::TargetShape::Point([p.lon, p.lat]),
    }
}

impl HookEchoApp {
    /// Start looking up the towns in `track`'s swath, unless that swath has been looked up.
    pub(crate) fn request_towns(
        &mut self,
        track: &crate::app::storm_track::ManualTrack,
        ctx: &egui::Context,
    ) {
        let key = track.impact_id();
        let book = &mut self.towns;
        if book.by_key.contains_key(&key) {
            return;
        }
        if book.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            book.tx = Some(tx);
            book.rx = Some(rx);
        }
        let tx = book.tx.clone().expect("set above");
        book.by_key.insert(key.clone(), TownsState::Pending);
        let rings = vec![track.swath()];
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let got = wxdata::census::places_in(&http, &rings, MAX_TOWNS)
                .await
                .map_err(|e| {
                    log::warn!("towns in the path: {e:#}");
                    e.to_string()
                });
            let _ = tx.send((key, got));
            ctx.request_repaint();
        });
    }

    /// Take finished lookups.
    pub(crate) fn sync_towns(&mut self) {
        self.towns.take_arrivals();
    }

    /// The towns found in `track`'s swath, as targets, and the lookup's state for the card.
    pub(crate) fn town_targets(
        &self,
        track: &crate::app::storm_track::ManualTrack,
    ) -> (Vec<crate::app::gis_layers::Target>, Option<&TownsState>) {
        let state = self.towns.state(&track.impact_id());
        let targets = match state {
            Some(TownsState::Ready(p)) => p.iter().map(place_target).collect(),
            _ => Vec::new(),
        };
        (targets, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_town_is_named_plainly_and_said_to_be_its_centre() {
        let t = place_target(&Place {
            geoid: "4019900".into(),
            name: "Del City city".into(),
            population: 21_822,
            lon: -97.44,
            lat: 35.448,
        });
        assert_eq!(t.name, "Del City, pop. 21,822");
        assert_eq!(t.layer, "Census 2020, town centre");
        assert_eq!(
            t.shape,
            crate::app::gis_layers::TargetShape::Point([-97.44, 35.448])
        );
    }
}
