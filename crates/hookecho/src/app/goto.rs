//! The `#goto=` / `hookecho://goto/` link: one format for a shared view, parsed and written here.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

/// URL scheme for a shared view. One parser serves all three ways a view arrives: the
/// `HOOKECHO_GOTO` env var, the `goto.txt` the Android notification tap writes (which uses the
/// site-less `,lon,lat,zoom` form), and a tapped `hookecho://goto/…` link.
pub(crate) const GOTO_SCHEME: &str = "hookecho://goto/";

/// A parsed deep link. `moment`/`tilt` stay `None` unless the link named them, so a link that
/// only says where to look leaves the product the viewer already had.
pub(crate) struct Goto {
    pub(crate) site: String,
    pub(crate) lon: f64,
    pub(crate) lat: f64,
    pub(crate) zoom: f64,
    pub(crate) time: Option<DateTime<Utc>>,
    pub(crate) moment: Option<Moment>,
    pub(crate) tilt: Option<usize>,
    pub(crate) basemap: Option<String>,
    /// Outer `None`: the link said nothing about the threshold, so the viewer keeps its own.
    /// Inner `None`: the link said `thr:off`, which turns the threshold off on purpose.
    pub(crate) threshold: Option<Option<f32>>,
    pub(crate) srv: bool,
    /// River gauges whose cards the link opens (`gauge:ACRT2`), turning the gauge layer on.
    pub(crate) gauges: Vec<String>,
    /// Tropical model guidance on (`tc`), optionally focused on one system (`tc:al062026`, which
    /// also opens its Models tab). `Some("")` is on with no focus.
    pub(crate) tropical: Option<String>,
}

/// Parse `[hookecho://goto/]SITE[,lon,lat,zoom][,extra…]`, where each extra is an RFC3339 time, a
/// moment code (`VEL`), a tilt index, a basemap (`bm:dark`), a threshold (`thr:25` / `thr:off`),
/// a river gauge whose card to open (`gauge:ACRT2`) or the literal `srv` — sniffed by shape, so their order does not matter. A bare `SITE` flies to the site itself; the site may be
/// empty when lon/lat are given.
pub(crate) fn parse_goto(v: &str) -> Option<Goto> {
    let v = v.trim().strip_prefix(GOTO_SCHEME).unwrap_or(v.trim());
    // Chat clients and mail readers hand the link back percent-encoded, commas and the time's
    // colons included, so decode the whole thing before splitting. No field here can legitimately
    // hold a comma, which is what makes decoding first the safe direction. Anything that isn't a
    // valid escape survives as typed, so a stray `%` doesn't lose the link.
    let decoded = percent_encoding::percent_decode_str(v).decode_utf8_lossy();
    let p: Vec<String> = decoded.split(',').map(|s| s.trim().to_string()).collect();
    let mut g = if p.len() == 1 {
        let s = wxdata::sites::site_by_id(&p[0])?;
        // Same zoom a cold start picks for the default site.
        Goto {
            site: p[0].to_ascii_uppercase(),
            lon: s.longitude as f64,
            lat: s.latitude as f64,
            zoom: 8.0,
            time: None,
            moment: None,
            tilt: None,
            basemap: None,
            threshold: None,
            srv: false,
            gauges: Vec::new(),
            tropical: None,
        }
    } else {
        let (Some(site), Some(Ok(lon)), Some(Ok(lat)), Some(Ok(zoom))) = (
            p.first(),
            p.get(1).map(|s| s.parse()),
            p.get(2).map(|s| s.parse()),
            p.get(3).map(|s| s.parse()),
        ) else {
            return None;
        };
        Goto {
            site: site.to_string(),
            lon,
            lat,
            zoom,
            time: None,
            moment: None,
            tilt: None,
            basemap: None,
            threshold: None,
            srv: false,
            gauges: Vec::new(),
            tropical: None,
        }
    };
    for s in p.iter().skip(4).filter(|s| !s.is_empty()) {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
            g.time = Some(t.with_timezone(&Utc));
        } else if let Some(m) = Moment::from_code(s) {
            g.moment = Some(m);
        } else if let Ok(i) = s.parse::<usize>() {
            g.tilt = Some(i);
        } else if let Some(slug) = s.strip_prefix("bm:") {
            g.basemap = Some(slug.to_string());
        } else if let Some(t) = s.strip_prefix("thr:") {
            // The value is in the moment's own unit, which is what the slider stores: dBZ for
            // reflectivity, m/s for velocity. Not the display unit — a link must mean the same
            // thing whichever Units the recipient has set.
            g.threshold = if t.eq_ignore_ascii_case("off") {
                Some(None)
            } else if let Ok(v) = t.parse::<f32>() {
                Some(Some(v))
            } else {
                log::warn!("goto: want thr:<number> or thr:off, got {s:?}");
                None
            };
        } else if s.eq_ignore_ascii_case("srv") {
            g.srv = true;
        } else if s.eq_ignore_ascii_case("tc") {
            g.tropical = Some(String::new());
        } else if let Some(id) = s.strip_prefix("tc:") {
            // An ATCF id: basin, number, year (`al062026`).
            if id.len() == 8 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                g.tropical = Some(id.to_ascii_lowercase());
            } else {
                log::warn!("goto: want tc:<ATCF id>, got {s:?}");
            }
        } else if let Some(lid) = s.strip_prefix("gauge:") {
            // An id is a few letters and digits; anything else is not a gauge.
            if !lid.is_empty() && lid.len() <= 8 && lid.chars().all(|c| c.is_ascii_alphanumeric()) {
                g.gauges.push(lid.to_ascii_uppercase());
            } else {
                log::warn!("goto: want gauge:<id>, got {s:?}");
            }
        } else {
            log::warn!("goto: ignoring unrecognized field {s:?}");
        }
    }
    Some(g)
}

/// The shareable link for a view. Native gets the `hookecho://` scheme the OS has registered; the
/// browser build gets its own origin with the state in the fragment, which never leaves the client
/// — no server, cache or worker ever sees where someone is looking.
pub(crate) fn goto_link(g: &Goto) -> String {
    let site = &g.site;
    let (lon, lat, zoom) = (g.lon, g.lat, g.zoom);
    let t = g
        .time
        .map(|t| format!(",{}", t.to_rfc3339()))
        .unwrap_or_default();
    // Defaults stay out of the link: a field that isn't there leaves the recipient's own alone,
    // which is the whole contract of the trailing fields.
    let m = match g.moment {
        Some(m) if m != Moment::Reflectivity => format!(",{}", m.short_name()),
        _ => String::new(),
    };
    let z = match g.tilt {
        Some(i) if i != 0 => format!(",{i}"),
        _ => String::new(),
    };
    // Only an active threshold travels. Sharing "no threshold" as `thr:off` would override the
    // recipient's own setting with a default nobody chose.
    let thr = g
        .threshold
        .flatten()
        .map(|v| format!(",thr:{v}"))
        .unwrap_or_default();
    let basemap = g
        .basemap
        .as_ref()
        .map(|s| format!(",bm:{s}"))
        .unwrap_or_default();
    let srv = if g.srv { ",srv" } else { "" };
    let gauges: String = g.gauges.iter().map(|l| format!(",gauge:{l}")).collect();
    let tc = match g.tropical.as_deref() {
        Some("") => ",tc".to_string(),
        Some(id) => format!(",tc:{id}"),
        None => String::new(),
    };
    let body =
        format!("{site},{lon:.4},{lat:.4},{zoom:.1}{t}{m}{z}{thr}{basemap}{srv}{gauges}{tc}");
    #[cfg(target_arch = "wasm32")]
    {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_default();
        format!("{origin}/#goto={body}")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        format!("{GOTO_SCHEME}{body}")
    }
}
