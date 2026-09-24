//! Case-study packages (ROADMAP_NEW K3): a portable JSON file that reopens an analysis — which
//! radars, which products, which moment in time and how long a replay around it, and the
//! annotations and bookmarks made along the way — on another machine or a year later.
//!
//! It is a [`Workspace`] (the pane arrangement, which the app already knows how to capture and
//! restore) plus what a workspace deliberately leaves out because it belongs to the moment: the
//! analysis time and replay window, bookmarks, markers, watch zones, freehand strokes and the
//! user-defined products the analysis used. Radar data itself is not packed — every volume the
//! case points at is public and refetched on open — so a case is a few kilobytes.
//!
//! Opening a case *adds* its bookmarks, markers, zones, strokes and products to what is already
//! there rather than replacing them: an analyst opening a colleague's case keeps their own.

use crate::settings::{AlertPolygon, Bookmark, Marker};
use crate::workspace::Workspace;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The manifest format this build writes, and the newest it reads.
pub const FORMAT: u32 = 1;

/// One freehand annotation, as stored: a `[lon, lat]` polyline and its colour as premultiplied
/// RGBA (egui's own `Color32` layout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseStroke {
    pub points: Vec<[f64; 2]>,
    pub rgba: [u8; 4],
}

/// Everything a case reopens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseManifest {
    /// [`FORMAT`] when written; a newer one than this build knows is refused on open.
    pub format: u32,
    pub name: String,
    /// The app version that wrote it, for a reader wondering why something did not come back.
    pub app_version: String,
    pub created_utc: DateTime<Utc>,
    /// The analysis instant every pane seeks to; `None` for a case saved while following live,
    /// which reopens live.
    pub time_utc: Option<DateTime<Utc>>,
    /// Replay window around `time_utc`, minutes; 0 is a still.
    pub span_min: u16,
    /// The radars the panes show, in pane order — for a reader of the file; `workspace` is what
    /// restores them.
    pub sites: Vec<String>,
    pub workspace: Workspace,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub zones: Vec<AlertPolygon>,
    #[serde(default)]
    pub strokes: Vec<CaseStroke>,
    #[serde(default)]
    pub udp_products: Vec<wxdata::udp::ProductDef>,
    /// Free text for whoever opens it next.
    #[serde(default)]
    pub notes: String,
}

impl CaseManifest {
    pub fn to_json(&self) -> String {
        // Serialising plain data cannot fail; an empty string would only hide a bug.
        serde_json::to_string_pretty(self).expect("a case manifest serialises")
    }

    /// Parse a manifest, refusing one written by a newer build (its fields may mean things this
    /// one would get wrong) and anything that is not a case at all.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let m: CaseManifest =
            serde_json::from_str(json).map_err(|e| format!("not a HookEcho case file: {e}"))?;
        if m.format > FORMAT {
            return Err(format!(
                "written by a newer HookEcho (case format {}, this build reads up to {FORMAT})",
                m.format
            ));
        }
        if m.workspace.panes.is_empty() {
            return Err("the case has no panes".to_string());
        }
        Ok(m)
    }

    /// A file name for this case: its name reduced to letters, digits and dashes.
    pub fn file_name(&self) -> String {
        let mut slug = String::new();
        for c in self.name.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        if slug.is_empty() {
            "case.hookecho.json".to_string()
        } else {
            format!("{slug}.hookecho.json")
        }
    }
}

/// The default name for a case: the first pane's radar and the analysis time, or "live".
pub fn default_name(sites: &[String], time: Option<DateTime<Utc>>) -> String {
    let site = sites.first().map_or("HookEcho", String::as_str);
    match time {
        Some(t) => format!("{site} {}", t.format("%Y-%m-%d %H:%MZ")),
        None => format!("{site} live"),
    }
}

/// Append each of `incoming` whose key is not already in `existing`; returns how many were added.
/// Opening a case twice, or a case that carries something the analyst already had, must not
/// duplicate it.
pub fn merge_by<T: Clone, K: PartialEq>(
    existing: &mut Vec<T>,
    incoming: &[T],
    key: impl Fn(&T) -> K,
) -> usize {
    let mut added = 0;
    for item in incoming {
        let k = key(item);
        if !existing.iter().any(|e| key(e) == k) {
            existing.push(item.clone());
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::PaneSnap;

    fn manifest() -> CaseManifest {
        let pane: PaneSnap = serde_json::from_value(serde_json::json!({
            "site": "KTLX", "moment": "Reflectivity", "tilt": 0, "srv": false,
            "basemap": "dark", "lon": -97.5, "lat": 35.3, "zoom": 9.0,
        }))
        .expect("a minimal pane snapshot");
        let workspace: Workspace = serde_json::from_value(serde_json::json!({
            "name": "case", "panes": [pane],
        }))
        .expect("a minimal workspace");
        let time = "2013-05-20T20:08:00Z".parse().unwrap();
        CaseManifest {
            format: FORMAT,
            name: default_name(&["KTLX".into()], Some(time)),
            app_version: "test".into(),
            created_utc: time,
            time_utc: Some(time),
            span_min: 60,
            sites: vec!["KTLX".into()],
            workspace,
            bookmarks: Vec::new(),
            markers: Vec::new(),
            zones: vec![AlertPolygon {
                name: "Moore".into(),
                ring: vec![[-97.5, 35.3], [-97.4, 35.3], [-97.4, 35.4]],
            }],
            strokes: vec![CaseStroke {
                points: vec![[-97.5, 35.3], [-97.45, 35.33]],
                rgba: [255, 80, 80, 255],
            }],
            udp_products: Vec::new(),
            notes: "Moore, EF5".into(),
        }
    }

    #[test]
    fn a_case_round_trips_and_is_named_for_its_radar_and_time() {
        let m = manifest();
        assert_eq!(m.name, "KTLX 2013-05-20 20:08Z");
        assert_eq!(m.file_name(), "ktlx-2013-05-20-20-08z.hookecho.json");
        let back = CaseManifest::from_json(&m.to_json()).unwrap();
        assert_eq!(back, m);
        assert_eq!(default_name(&[], None), "HookEcho live");
    }

    #[test]
    fn a_newer_format_or_an_empty_or_foreign_file_is_refused() {
        let mut m = manifest();
        m.format = FORMAT + 1;
        let e = CaseManifest::from_json(&m.to_json()).unwrap_err();
        assert!(e.contains("newer HookEcho"), "{e}");
        let mut empty = manifest();
        empty.workspace.panes.clear();
        assert!(CaseManifest::from_json(&empty.to_json()).is_err());
        let e = CaseManifest::from_json(r#"{"default_site":"KTLX"}"#).unwrap_err();
        assert!(e.contains("not a HookEcho case"), "{e}");
    }

    #[test]
    fn a_case_written_before_the_optional_lists_existed_still_opens() {
        let mut v: serde_json::Value = serde_json::from_str(&manifest().to_json()).unwrap();
        for k in [
            "bookmarks",
            "markers",
            "zones",
            "strokes",
            "udp_products",
            "notes",
        ] {
            v.as_object_mut().unwrap().remove(k);
        }
        let m = CaseManifest::from_json(&v.to_string()).unwrap();
        assert!(m.zones.is_empty() && m.notes.is_empty());
    }

    #[test]
    fn merging_adds_only_what_is_not_already_there() {
        let mut have = vec![("a", 1), ("b", 2)];
        let added = merge_by(&mut have, &[("b", 9), ("c", 3), ("c", 4)], |x| x.0);
        assert_eq!(added, 1);
        assert_eq!(have, vec![("a", 1), ("b", 2), ("c", 3)]);
        // Opening the same case again adds nothing.
        assert_eq!(merge_by(&mut have, &[("c", 3)], |x| x.0), 0);
    }
}
