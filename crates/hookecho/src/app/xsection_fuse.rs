//! The cross-section with nearby radars added (ROADMAP_NEW R1, 1010.md V18): the window's
//! "Add nearby radars" switch fuses the pane's reflectivity with up to three other WSR-88Ds
//! within 230 km of the section's middle ([`wxdata::multiradar::fuse_section`]). Each radar's
//! volume nearest the pane's is fetched (through the disk cache) and must be within five
//! minutes of it. The fused panel replaces the single-radar one only for the same pane, volume
//! and line. The window's line says which radars went in, their offsets, and any left out.
//! Reflectivity only: radial velocities are never combined.

use super::*;

/// Most radars added to the pane's own.
pub(super) const MAX_ADDED: usize = 3;
/// How far from the section's middle an added radar may be, km.
pub(super) const MAX_RANGE_KM: f64 = 230.0;
/// How far an added radar's volume may be from the pane's, s.
pub(super) const MAX_OFFSET_S: i64 = 300;

/// An added radar: its site, its volume's time, its antenna's height (m) and its reflectivity
/// tilts.
pub(super) type Added = (String, DateTime<Utc>, f64, Vec<wxdata::level2::BinnedSweep>);

/// What a fused section was asked for: the pane, its volume, and the line's ends.
type FusionKey = (usize, String, [f64; 4]);

/// A fused section, or why there is none, for the request it answers.
type FusionReply = (FusionKey, Result<wxdata::multiradar::FusedSection, String>);

#[derive(Default)]
pub(crate) struct XsFusion {
    /// The window's switch.
    pub on: bool,
    /// What the window says about it: working, which radars went in, or why none did.
    pub status: Option<String>,
    rx: Option<std::sync::mpsc::Receiver<FusionReply>>,
    pending: Option<FusionKey>,
    /// The last request started, so a line that has stopped moving is fused once.
    attempted: Option<FusionKey>,
}

/// The other WSR-88Ds near `(lon, lat)`, nearest beam first, at most `limit`, not `skip`.
pub(super) fn nearby_radars(lon: f64, lat: f64, skip: &str, limit: usize) -> Vec<&'static str> {
    wxdata::suitability::rank(lon, lat, 0.5, 24)
        .into_iter()
        .filter(|c| c.distance_km <= MAX_RANGE_KM)
        .map(|c| c.site.id)
        .filter(|id| wxdata::sites::is_nexrad(id) && !id.eq_ignore_ascii_case(skip))
        .take(limit)
        .collect()
}

/// A radar's antenna height above sea level, m: the site's ground and its tower.
pub(super) fn antenna_m(id: &str) -> f64 {
    wxdata::sites::site_by_id(id).map_or(0.0, |s| {
        f64::from(s.elevation_meters) + wxdata::towers::tower_m(s.id)
    })
}

/// Each of `sites`' volume nearest `time`, if within [`MAX_OFFSET_S`] of it, through the disk
/// cache. A radar with no such volume, or whose fetch fails, is skipped (and logged under
/// `what`).
pub(super) async fn fetch_added(
    sites: Vec<String>,
    time: DateTime<Utc>,
    cache: Option<std::path::PathBuf>,
    what: &str,
) -> Vec<(String, DateTime<Utc>, f64, wxdata::level2::Scan)> {
    let mut scans = Vec::new();
    for site in sites {
        let Ok(ids) = wxdata::level2::list_volumes(&site, time.date_naive()).await else {
            continue;
        };
        let Some(id) = ids
            .into_iter()
            .filter(|id| id.date_time().is_some())
            .min_by_key(|id| (id.date_time().unwrap() - time).num_seconds().abs())
        else {
            continue;
        };
        let vol_time = id.date_time().unwrap();
        if (vol_time - time).num_seconds().abs() > MAX_OFFSET_S {
            continue;
        }
        let antenna = antenna_m(&site);
        match crate::volume::fetch(id, true, cache.clone()).await {
            Ok(scan) => scans.push((site, vol_time, antenna, scan)),
            Err(e) => log::warn!("{what}: {site}: {e:#}"),
        }
    }
    scans
}

/// The fetched radars' reflectivity tilts, binned (off the UI thread where there is one).
pub(super) fn bin_added(
    scans: Vec<(String, DateTime<Utc>, f64, wxdata::level2::Scan)>,
) -> Vec<Added> {
    scans
        .into_iter()
        .map(|(site, t, antenna_m, scan)| {
            let tilts = (0..wxdata::level2::elevation_angles(&scan).len())
                .filter_map(|tilt| {
                    wxdata::level2::bin_scan_opts_recorded(&scan, Moment::Reflectivity, tilt, false)
                        .ok()
                })
                .collect();
            (site, t, antenna_m, tilts)
        })
        .collect()
}

/// The window's line about a fused section.
fn fused_status(f: &wxdata::multiradar::FusedSection) -> String {
    let added: Vec<String> = f
        .sites
        .iter()
        .zip(&f.offsets_s)
        .skip(1)
        .map(|(s, o)| format!("{s} ({o:+} s)"))
        .collect();
    let mut s = if added.is_empty() {
        "No nearby radar had a volume close enough in time; the panel is this radar's alone"
            .to_string()
    } else {
        format!(
            "Fused with {}: reflectivity weighted by each beam's thickness, never between beams",
            added.join(", ")
        )
    };
    for l in &f.left_out {
        s.push_str(&format!("; {} left out: {}", l.site, l.reason));
    }
    s
}

impl HookEchoApp {
    /// After the single-radar section is built: fetch and fuse the nearby radars for it, if the
    /// switch is on. `sweeps` are the pane's reflectivity tilts, `time` its volume's time.
    #[allow(clippy::too_many_arguments)] // the pane, the line's two ends, its data and time
    pub(crate) fn start_xs_fusion(
        &mut self,
        idx: usize,
        a: [f64; 2],
        b: [f64; 2],
        sweeps: Vec<wxdata::level2::BinnedSweep>,
        time: DateTime<Utc>,
        ctx: &egui::Context,
    ) {
        self.xs_fusion.pending = None;
        self.xs_fusion.rx = None;
        if !self.xs_fusion.on {
            self.xs_fusion.status = None;
            return;
        }
        if self.xsection_moment != Moment::Reflectivity {
            self.xs_fusion.status =
                Some("Nearby radars add reflectivity only; velocities are never combined".into());
            return;
        }
        let Some(site) = self.views[idx].site.clone() else {
            return;
        };
        let Some(volume) = self.views[idx].volume.as_ref().map(|v| v.name.clone()) else {
            return;
        };
        // Not while the line is being dragged: once it stops, [`Self::poll_xs_fusion`] asks again.
        if self.xsection_drag.is_some() {
            self.xs_fusion.status =
                Some("Nearby radars are added when the line stops moving".into());
            return;
        }
        // Recorded before anything below can give up, so the per-frame check asks only once.
        let key: FusionKey = (idx, volume, [a[0], a[1], b[0], b[1]]);
        self.xs_fusion.attempted = Some(key.clone());
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let others = nearby_radars(mid[0], mid[1], &site, MAX_ADDED);
        if others.is_empty() {
            self.xs_fusion.status = Some(format!("No other radar within {MAX_RANGE_KM:.0} km"));
            return;
        }
        self.xs_fusion.status = Some(format!("Adding {}\u{2026}", others.join(", ")));
        self.xs_fusion.pending = Some(key.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        self.xs_fusion.rx = Some(rx);
        let own = (site.clone(), antenna_m(&site));
        let others: Vec<String> = others.into_iter().map(str::to_string).collect();
        let cache = crate::paths::cache_dir();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let scans = fetch_added(others, time, cache, "cross-section fusion").await;
            let result = wxdata::task::blocking(move || {
                let added = bin_added(scans);
                let mut radars = vec![wxdata::multiradar::RadarVolume {
                    site: own.0.clone(),
                    time,
                    antenna_m: own.1,
                    sweeps: &sweeps,
                }];
                radars.extend(added.iter().map(|(site, t, antenna_m, tilts)| {
                    wxdata::multiradar::RadarVolume {
                        site: site.clone(),
                        time: *t,
                        antenna_m: *antenna_m,
                        sweeps: tilts,
                    }
                }));
                wxdata::multiradar::fuse_section(
                    &radars,
                    time,
                    MAX_OFFSET_S,
                    (a[0], a[1]),
                    (b[0], b[1]),
                    300,
                    120,
                    18.0,
                )
                .ok_or_else(|| "nothing to fuse".to_string())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            let _ = tx.send((key, result));
            ctx.request_repaint();
        });
    }

    /// Take a finished fusion: it replaces the panel only for the same pane, volume and line.
    /// With the switch on and nothing running, a line that stopped moving since the last request
    /// is fused once.
    pub(crate) fn poll_xs_fusion(&mut self, ctx: &egui::Context) {
        if self.xs_fusion.on
            && self.xs_fusion.rx.is_none()
            && self.xsection_drag.is_none()
            && self.xsection_moment == Moment::Reflectivity
        {
            if let (Some(src), [a, b]) =
                (self.xsection_source.as_ref(), self.xsection_pts.as_slice())
            {
                let key: FusionKey = (src.pane, src.volume.clone(), [a[0], a[1], b[0], b[1]]);
                if self.xs_fusion.attempted.as_ref() != Some(&key) {
                    let pane = src.pane;
                    self.build_xsection(pane, ctx);
                    return;
                }
            }
        }
        let Some(rx) = &self.xs_fusion.rx else {
            return;
        };
        let Ok((key, result)) = rx.try_recv() else {
            return;
        };
        self.xs_fusion.rx = None;
        if self.xs_fusion.pending.as_ref() != Some(&key) {
            return;
        }
        self.xs_fusion.pending = None;
        let current = self
            .xsection_source
            .as_ref()
            .map(|s| (s.pane, s.volume.clone()));
        let line = match self.xsection_pts.as_slice() {
            [a, b] => Some([a[0], a[1], b[0], b[1]]),
            _ => None,
        };
        if current != Some((key.0, key.1.clone())) || line != Some(key.2) {
            return;
        }
        match result {
            Ok(fused) => {
                self.xs_fusion.status = Some(fused_status(&fused));
                let table = crate::colormap::effective_table(
                    &self.palettes,
                    Moment::Reflectivity,
                    self.settings.theme,
                );
                let img = ui::xsection_window::to_image(&fused.section, &table);
                self.xsection_tex =
                    Some(ctx.load_texture("xsection", img, egui::TextureOptions::LINEAR));
                self.xsection = Some(fused.section);
            }
            Err(e) => self.xs_fusion.status = Some(format!("Nearby radars not added: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Around Moore the nearby radars are other WSR-88Ds within range, not the pane's own, and
    /// the window's line names them with their offsets and any left out.
    #[test]
    fn nearby_radars_are_others_in_range_and_the_line_names_them() {
        let near = nearby_radars(-97.50, 35.33, "KTLX", 3);
        assert_eq!(near.len(), 3, "{near:?}");
        assert!(near
            .iter()
            .all(|s| *s != "KTLX" && wxdata::sites::is_nexrad(s)));
        let ranked = wxdata::suitability::rank(-97.50, 35.33, 0.5, 24);
        for s in &near {
            let d = ranked
                .iter()
                .find(|c| c.site.id == *s)
                .map(|c| c.distance_km)
                .unwrap();
            assert!(d <= MAX_RANGE_KM, "{s} {d}");
        }
        let f = wxdata::multiradar::FusedSection {
            section: wxdata::xsection::CrossSection {
                cols: 2,
                rows: 2,
                max_height_km: 18.0,
                length_km: 10.0,
                dbz: vec![None; 4],
                beam_covered: vec![false; 4],
                beam_lines: Vec::new(),
            },
            sites: vec!["KTLX".into(), "KFDR".into()],
            offsets_s: vec![0, -31],
            left_out: vec![wxdata::multiradar::LeftOut {
                site: "KINX".into(),
                reason: "its volume is 400 s from the time asked for".into(),
            }],
            dominant: vec![u8::MAX; 4],
        };
        let line = fused_status(&f);
        assert!(line.starts_with("Fused with KFDR (-31 s)"), "{line}");
        assert!(
            line.contains("KINX left out: its volume is 400 s"),
            "{line}"
        );
    }
}
