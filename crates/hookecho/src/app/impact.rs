//! Population and impact for alerts and discussions: the people, homes and towns inside an open
//! alert's (or discussion's) polygon, fetched once per alert from the Census
//! ([`wxdata::census::impact`]) while its card is open, and kept for the session.
//!
//! Its own module rather than more of `app.rs` (ROADMAP_NEW 2.1).

use super::HookEchoApp;
use std::collections::HashMap;
use wxdata::census::Impact;

/// One area's impact, by the id the card knows it by.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ImpactState {
    Pending,
    Ready(Impact),
    Failed,
}

#[derive(Default)]
pub(crate) struct ImpactBook {
    pub(crate) by_id: HashMap<String, ImpactState>,
    rx: Option<std::sync::mpsc::Receiver<(String, Option<Impact>)>>,
    tx: Option<std::sync::mpsc::Sender<(String, Option<Impact>)>>,
}

impl ImpactBook {
    fn take_arrivals(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok((id, impact)) = rx.try_recv() {
                let state = impact.map_or(ImpactState::Failed, ImpactState::Ready);
                self.by_id.insert(id, state);
            }
        }
    }
}

/// "281,434".
pub(crate) fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The short line a card shows: "About 281,434 people · 129,785 homes".
pub(crate) fn summary(i: &Impact) -> String {
    format!(
        "About {} people \u{b7} {} homes",
        thousands(i.population),
        thousands(i.housing_units)
    )
}

/// The towns line: "Oklahoma City (681,054), Midwest City (58,409)", whole-town populations.
pub(crate) fn towns(i: &Impact) -> String {
    i.places
        .iter()
        .map(|(name, pop)| {
            // "Oklahoma City city" / "Forest Park town" / "Jones CDP": the Census's own kind suffix.
            let name = ["city", "town", "village", "borough", "CDP"]
                .iter()
                .find_map(|k| name.strip_suffix(&format!(" {k}")))
                .unwrap_or(name);
            format!("{name} ({})", thousands(*pop))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl HookEchoApp {
    /// Take finished impact lookups, and start one for each open alert card that has none.
    pub(crate) fn sync_impacts(&mut self, ctx: &egui::Context) {
        self.impacts.take_arrivals();
        self.sync_towns();
        let Some(ids) = self.warning_popup.as_ref().map(|p| {
            p.cards
                .iter()
                .map(|c| c.info.id.clone())
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        for id in ids {
            if self.impacts.by_id.contains_key(&id) {
                continue;
            }
            let rings: Vec<Vec<[f64; 2]>> = self
                .active_alert_features()
                .iter()
                .filter(|f| f.alert.as_ref().is_some_and(|a| a.id == id))
                .flat_map(|f| f.rings.iter().cloned())
                .collect();
            if rings.is_empty() {
                self.impacts.by_id.insert(id, ImpactState::Failed);
                continue;
            }
            self.request_impact(id, rings, ctx);
        }
    }

    /// Look up the people inside `rings`, filed under `id`.
    pub(crate) fn request_impact(
        &mut self,
        id: String,
        rings: Vec<Vec<[f64; 2]>>,
        ctx: &egui::Context,
    ) {
        let book = &mut self.impacts;
        if book.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            book.tx = Some(tx);
            book.rx = Some(rx);
        }
        let tx = book.tx.clone().expect("set above");
        book.by_id.insert(id.clone(), ImpactState::Pending);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let got = match wxdata::census::impact(&http, &rings).await {
                Ok(i) => Some(i),
                Err(e) => {
                    log::warn!("impact for {id}: {e:#}");
                    None
                }
            };
            let _ = tx.send((id, got));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_towns_read_plainly() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(281_434), "281,434");
        assert_eq!(thousands(1_234_567), "1,234,567");
        let i = Impact {
            population: 281_434,
            housing_units: 129_785,
            places: vec![
                ("Oklahoma City city".into(), 681_054),
                ("Forest Park town".into(), 1_049),
                ("Jones CDP".into(), 3_000),
            ],
        };
        assert_eq!(summary(&i), "About 281,434 people \u{b7} 129,785 homes");
        assert_eq!(
            towns(&i),
            "Oklahoma City (681,054), Forest Park (1,049), Jones (3,000)"
        );
    }
}
