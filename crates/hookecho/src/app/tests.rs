#[test]
fn site_id_round_trips_through_the_palette_action_buffer() {
    use super::{decode_site_id, encode_site_id};
    for id in ["KTLX", "TADW", "DEAS", "BEHEL"] {
        assert_eq!(decode_site_id(encode_site_id(id)), id);
    }
}

#[test]
fn an_id_shorter_than_the_buffer_does_not_carry_trailing_junk() {
    use super::{decode_site_id, encode_site_id};
    // The buffer is zero-padded; a 4-character id must not decode with the 4 trailing zero
    // bytes read back as anything other than "the string ends here".
    assert_eq!(decode_site_id(encode_site_id("KTLX")).len(), 4);
}

/// ROADMAP_2 §7.5: this file only gets smaller. A new feature belongs in its own module under
/// `app/`; when an extraction lands, lower the ceiling to the new length so it stays down.
#[test]
fn app_rs_only_gets_smaller() {
    const CEILING: usize = 8969;
    let lines = include_str!("../app.rs").lines().count();
    assert!(
        lines <= CEILING,
        "app.rs grew to {lines} lines (ceiling {CEILING}): put the new code in a module under app/ instead"
    );
}

#[test]
fn a_source_says_how_it_recovers() {
    let h = super::SourceHealth {
        source: "Weather alerts".into(),
        endpoint_family: crate::source_health::EndpointFamily::NwsApi,
        latest_valid_time: None,
        fallback_providers: Vec::new(),
        cache_state: super::CacheState::Memory,
        fetching: false,
        last_attempt: None,
        last_success: None,
        last_failure: None,
        error: None,
        cadence: std::time::Duration::from_secs(120),
        selection_only: false,
        recent_outcomes: None,
        details: Vec::new(),
        severity: crate::source_health::FeedSource::WeatherAlerts.severity(),
    };
    let r = h.recovery();
    assert!(r.contains("Retried every 2m"), "{r}");
    assert!(r.contains("stale after 4m"), "{r}");
    assert!(r.contains("Severity: critical"), "{r}");
}

#[test]
fn wheel_and_trackpad_zoom_count_as_live_gestures() {
    let mut input = egui::InputState::default();
    assert!(!super::map_gesture_live(&input));
    input.smooth_scroll_delta = egui::vec2(0.0, 0.25);
    assert!(
        super::map_gesture_live(&input),
        "include the smoothed wheel tail"
    );
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        events: vec![egui::Event::Zoom(1.1)],
        ..Default::default()
    };
    let _ = ctx.run_ui(raw, |ui| {
        assert!(
            ui.input(super::map_gesture_live),
            "trackpad pinch without a button"
        );
    });
}

/// New geometry appears when it lands; a zoom-bucket crossing waits for the finger to lift,
/// so a pinch across three buckets tessellates once instead of three times.
#[test]
fn a_pinch_defers_the_retessellation_but_new_geometry_never_waits() {
    use super::should_retess;
    assert!(
        should_retess(false, false, true),
        "quiet: rebuild on a bucket change"
    );
    assert!(!should_retess(true, false, true), "mid-gesture: wait");
    assert!(
        should_retess(true, true, true),
        "new geometry mid-gesture is data arriving, not the camera moving"
    );
    assert!(!should_retess(true, false, false));
    assert!(
        !should_retess(false, false, false),
        "nothing changed, nothing to do"
    );
}

/// A palette change must not re-send the sweep. Everything the GPU keeps (the gate bytes,
/// the precipitation-flag grid) is left out of a LUT-only upload; the color table and the
/// uniform, which are what a re-bake changes, are still there in full.
#[test]
fn a_palette_change_uploads_the_table_and_nothing_else() {
    let sweep = wxdata::level2::BinnedSweep {
        moment: wxdata::level2::Moment::Reflectivity,
        az_bins: 360,
        gate_count: 200,
        data: vec![7u8; 360 * 200],
        first_gate_km: 2.125,
        gate_interval_km: 0.25,
        radar_lat: 35.0,
        radar_lon: -97.0,
        elevation_deg: 0.5,
        value_min: -32.0,
        value_max: 95.0,
        ..Default::default()
    };
    let table = crate::colormap::default_table(wxdata::level2::Moment::Reflectivity);
    let full = super::to_upload(&sweep, table, None, false, None, None, false, None);
    let lut = super::to_upload(&sweep, table, None, false, None, None, true, None);
    assert_eq!(full.data.len(), 360 * 200);
    assert!(lut.data.is_empty(), "the gate texture is already uploaded");
    assert_eq!(lut.lut, full.lut, "the color table is what changed");
    assert_eq!(lut.uniform, full.uniform);
    assert_eq!((lut.az_bins, lut.gate_count), (360, 200));
    assert!(lut.lut_only);
}

/// The DWD arm exists because DL-DE/BY-2.0 requires the credit; the NOAA sites must stay
/// uncredited so the corner is empty on the maps most users open.
#[test]
fn only_licences_that_ask_for_a_credit_get_one() {
    assert_eq!(
        super::data_attribution("DEBO"),
        Some("Radar data © Deutscher Wetterdienst (DL-DE/BY-2.0)")
    );
    assert_eq!(super::data_attribution("KTLX"), None);
}

/// Scrubbing reverses constantly, so the paused set has to reach backwards as well; playback
/// only ever goes forward. Nearest frames come first either way, because the in-flight budget
/// usually runs out before the list does.
#[test]
fn prefetch_reaches_backwards_only_when_paused() {
    let playing = super::prefetch_offsets(true);
    assert!(playing.iter().all(|d| *d > 0), "playback never looks back");
    let paused = super::prefetch_offsets(false);
    assert!(paused.contains(&-1), "scrubbing back a frame must be warm");
    assert_eq!(paused[0], 1, "nearest frame first");
    for set in [playing, paused] {
        let mut sorted = set.to_vec();
        sorted.sort_by_key(|d| d.abs());
        assert_eq!(set, sorted.as_slice(), "nearest-first order");
    }
}

#[test]
fn goto_extras_carry_basemap_and_srv() {
    // New extras, in either order, alongside the old ones.
    let g = super::parse_goto("KTLX,-97.3,35.3,6.5,bm:dark,srv,VEL,2").unwrap();
    assert_eq!(g.site, "KTLX");
    assert_eq!(g.basemap.as_deref(), Some("dark"));
    assert!(g.srv);
    assert_eq!(g.tilt, Some(2));
    let round = super::parse_goto(&super::goto_link(&g)).unwrap();
    assert_eq!(round.basemap, g.basemap);
    assert_eq!(round.srv, g.srv);
    assert_eq!(round.moment, g.moment);
    assert_eq!(round.tilt, g.tilt);
    // Gauges ride along, validated, and come back from the link they went into.
    let g = super::parse_goto("KGRK,-97.7,30.2,9,gauge:acrt2,gauge:../x,gauge:BRTT2").unwrap();
    assert_eq!(g.gauges, ["ACRT2", "BRTT2"]);
    let link = super::goto_link(&g);
    assert!(link.ends_with(",gauge:ACRT2,gauge:BRTT2"), "{link}");
    assert_eq!(super::parse_goto(&link).unwrap().gauges, g.gauges);
    // Tropical guidance: on, or focused on a system; junk ids are refused.
    let g = super::parse_goto("KAMX,-80.4,25.6,6,tc:AL062026").unwrap();
    assert_eq!(g.tropical.as_deref(), Some("al062026"));
    assert!(super::goto_link(&g).ends_with(",tc:al062026"));
    let g = super::parse_goto("KAMX,-80.4,25.6,6,tc").unwrap();
    assert_eq!(g.tropical.as_deref(), Some(""));
    assert_eq!(
        super::parse_goto(&super::goto_link(&g)).unwrap().tropical,
        g.tropical
    );
    assert_eq!(
        super::parse_goto("KAMX,-80.4,25.6,6,tc:x/y")
            .unwrap()
            .tropical,
        None
    );
    // Old links are unchanged: no basemap, not storm-relative.
    let g = super::parse_goto(",-97.3,35.3,6.5").unwrap();
    assert_eq!(g.site, "");
    assert_eq!(g.basemap, None);
    assert!(!g.srv);
    assert_eq!(g.zoom, 6.5);
}

#[test]
fn goes_frame_follows_the_radar_clock() {
    use chrono::{TimeZone, Utc};
    let t = |m: u32| Utc.with_ymd_and_hms(2026, 5, 1, 20, m, 0).unwrap();
    let times = [t(0), t(10), t(20), t(30)];
    // Nearest wins, ties included.
    assert_eq!(super::nearest_goes(&times, t(21)), Some(t(20)));
    assert_eq!(super::nearest_goes(&times, t(26)), Some(t(30)));
    // Past the tolerance, stay on the latest instead of showing the wrong hour.
    let far = Utc.with_ymd_and_hms(2026, 5, 1, 22, 0, 0).unwrap();
    assert_eq!(super::nearest_goes(&times, far), None);
    assert_eq!(super::nearest_goes(&[], t(0)), None);
}

#[test]
fn temperature_contours_follow_the_selected_unit() {
    use crate::settings::TempUnit;

    for kind in [ContourKind::T2m, ContourKind::Td2m] {
        assert_eq!(kind.interval(TempUnit::Fahrenheit), 5.0);
        assert_eq!(kind.interval(TempUnit::Celsius), 2.0);
        assert!((kind.to_display(273.15, TempUnit::Fahrenheit) - 32.0).abs() < 1e-4);
        assert!(kind.to_display(273.15, TempUnit::Celsius).abs() < 1e-4);
        assert!(kind.display_label(TempUnit::Fahrenheit).ends_with("°F"));
        assert!(kind.display_label(TempUnit::Celsius).ends_with("°C"));
    }

    let model = wxdata::hrrr::Model::Hrrr;
    assert_ne!(
        (ContourKind::T2m, model, TempUnit::Fahrenheit),
        (ContourKind::T2m, model, TempUnit::Celsius),
        "the fetch key must change with units"
    );
    assert_eq!(ContourKind::Mslp.interval(TempUnit::Celsius), 2.0);
}

#[test]
fn contour_summary_names_what_is_actually_on() {
    let mut active = std::collections::BTreeSet::new();
    assert_eq!(summarize_contours(&active), "Off");
    active.insert(ContourKind::Mslp);
    assert_eq!(summarize_contours(&active), "MSLP");
    // Order follows the enum's own declaration order (`BTreeSet`'s derived `Ord`), not
    // insertion order, so the summary reads the same regardless of which was toggled first.
    active.insert(ContourKind::Cape);
    assert_eq!(summarize_contours(&active), "MSLP, SB-CAPE");
    let mut inserted_other_order = std::collections::BTreeSet::new();
    inserted_other_order.insert(ContourKind::Cape);
    inserted_other_order.insert(ContourKind::Mslp);
    assert_eq!(summarize_contours(&inserted_other_order), "MSLP, SB-CAPE");
}

#[test]
fn blink_alternates_on_a_clean_half_cycle_boundary() {
    // A is up for the first half of every cycle, B for the second — starting on A at t=0.
    assert!(!HookEchoApp::blink_showing_b(0.0, 1.5));
    assert!(!HookEchoApp::blink_showing_b(1.49, 1.5));
    assert!(HookEchoApp::blink_showing_b(1.5, 1.5));
    assert!(HookEchoApp::blink_showing_b(2.9, 1.5));
    // A second full cycle later, the phase repeats rather than drifting.
    assert!(!HookEchoApp::blink_showing_b(3.0, 1.5));
    assert!(HookEchoApp::blink_showing_b(4.5, 1.5));
}

#[test]
fn swipe_side_tracks_the_clamped_divider() {
    assert!(!HookEchoApp::swipe_showing_b(0.5, 499.9, 1000.0));
    assert!(HookEchoApp::swipe_showing_b(0.5, 500.0, 1000.0));
    assert!(!HookEchoApp::swipe_showing_b(-1.0, -0.1, 1000.0));
    assert!(HookEchoApp::swipe_showing_b(-1.0, 0.0, 1000.0));
    assert!(!HookEchoApp::swipe_showing_b(2.0, 999.9, 1000.0));
    assert!(HookEchoApp::swipe_showing_b(2.0, 1000.0, 1000.0));
}

fn scan_progress(chunk_index: usize, chunks_in_sweep: usize) -> wxdata::live::ScanProgress {
    wxdata::live::ScanProgress {
        volume_start_ms: None,
        vcp_number: None,
        cut_kind: wxdata::live::CutKind::Standard,
        elevation_number: 2,
        total_elevations: 12,
        elevation_angle_deg: 0.9,
        azimuth_rate_dps: 90.0,
        azimuth_start_deg: (chunk_index.saturating_sub(1) as f64 * 360.0)
            / chunks_in_sweep.max(1) as f64,
        azimuth_end_deg: (chunk_index as f64 * 360.0) / chunks_in_sweep.max(1) as f64,
        chunk_index,
        chunks_in_sweep,
    }
}

#[test]
fn live_sweep_animates_only_the_arriving_chunk_sector() {
    let progress = scan_progress(2, 3);
    let duration = progress.chunk_duration_secs();
    let start = super::live_sweep_frame(progress, 0.0, false).unwrap();
    assert!((start.angle_deg - 120.0).abs() < 1e-4);
    assert_eq!(start.alpha, 1.0);

    let middle = super::live_sweep_frame(progress, duration * 0.5, false).unwrap();
    assert!((middle.angle_deg - 180.0).abs() < 1e-3);
    let end = super::live_sweep_frame(progress, duration, false).unwrap();
    assert!((end.angle_deg - 240.0).abs() < 1e-3);
    assert!(super::live_sweep_frame(progress, duration + 0.36, false).is_none());

    let mut slower_cut = scan_progress(2, 3);
    slower_cut.azimuth_rate_dps = 60.0;
    let slower_middle =
        super::live_sweep_frame(slower_cut, slower_cut.chunk_duration_secs() * 0.5, false).unwrap();
    assert!((slower_middle.angle_deg - 180.0).abs() < 1e-3);
}

#[test]
fn live_sweep_wraps_north_and_reduced_motion_holds_the_arrived_edge() {
    let moving = super::live_sweep_frame(scan_progress(6, 6), 0.0, false).unwrap();
    assert!((moving.angle_deg - 300.0).abs() < 1e-4);
    let reduced = super::live_sweep_frame(scan_progress(6, 6), 0.0, true).unwrap();
    assert!(reduced.angle_deg.abs() < 1e-4);
    assert!(reduced.remaining_secs > 0.0);
}

#[test]
fn live_sweep_rejects_bad_metadata_and_other_tilts() {
    assert!(super::live_sweep_frame(scan_progress(0, 6), 0.0, false).is_none());
    assert!(super::live_sweep_frame(scan_progress(7, 6), 0.0, false).is_none());
    assert!(super::live_sweep_frame(scan_progress(1, 0), 0.0, false).is_none());
    assert!(super::live_progress_matches_tilt(scan_progress(1, 6), 0.91));
    assert!(!super::live_progress_matches_tilt(scan_progress(1, 6), 1.3));
}

#[test]
fn live_sweep_paints_keyed_beam_and_tail() {
    let ctx = egui::Context::default();
    let size = egui::vec2(400.0, 300.0);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let painter = ui.painter_at(rect);
        let camera = crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 6.0);
        super::paint_live_sweep(
            &painter,
            rect,
            camera,
            (size.x, size.y),
            [-97.0, 35.0],
            super::LiveSweepFrame {
                angle_deg: 90.0,
                alpha: 1.0,
                remaining_secs: 1.0,
            },
        );
    });
    let segments = output
        .shapes
        .iter()
        .filter(|shape| matches!(shape.shape, egui::Shape::LineSegment { .. }))
        .count();
    assert_eq!(segments, 4, "two trail lines plus the keyed live beam");
}

#[test]
fn blink_schedules_the_next_repaint_exactly_at_the_flip() {
    assert_eq!(HookEchoApp::blink_seconds_until_flip(0.0, 1.5), 1.5);
    assert!((HookEchoApp::blink_seconds_until_flip(1.0, 1.5) - 0.5).abs() < 1e-9);
    assert!((HookEchoApp::blink_seconds_until_flip(1.5, 1.5) - 1.5).abs() < 1e-9);
    // Never schedules a zero or negative delay, which would either spin every frame or panic
    // the `Duration` conversion at the call site.
    assert!(HookEchoApp::blink_seconds_until_flip(1.499_999_999, 1.5) > 0.0);
}

#[test]
fn icon_sheet_cache_paths_are_per_url_and_filename_safe() {
    let Some(a) = super::icon_sheet_cache_path("https://example.com/a/icons.png") else {
        return; // no disk on this platform: nothing to name
    };
    let b = super::icon_sheet_cache_path("https://example.com/b/icons.png").unwrap();
    assert_ne!(a, b, "two sheets must not share a file");
    let name = a.file_name().unwrap().to_str().unwrap();
    assert!(
        name.chars().all(|c| c.is_ascii_hexdigit()),
        "a URL is not a filename: got {name}"
    );
}
use super::*;

#[test]
fn every_overlay_toggle_survives_a_slug_round_trip() {
    for t in OverlayToggle::ALL {
        assert_eq!(OverlayToggle::from_slug(&t.slug()), Some(t), "{t:?}");
    }
    // ALL has to actually be all of them — a variant left out would silently stop persisting.
    let mut slugs: Vec<String> = OverlayToggle::ALL.iter().map(|t| t.slug()).collect();
    slugs.sort();
    slugs.dedup();
    assert_eq!(slugs.len(), OverlayToggle::ALL.len());
    assert_eq!(OverlayToggle::from_slug("Teleportation"), None);
}

#[test]
fn the_separate_rotation_and_debris_layers_restore_as_tornado_detection() {
    // Rotation couplets, debris signatures and Tornado ID are one feature now. A settings file,
    // workspace or scene saved with either detector layer on turns Tornado detection on, and the
    // old "one detection per tornado" switch is simply gone.
    assert_eq!(
        OverlayToggle::from_slug("Couplets"),
        Some(OverlayToggle::TornadoId)
    );
    assert_eq!(
        OverlayToggle::from_slug("Tds"),
        Some(OverlayToggle::TornadoId)
    );
    assert_eq!(OverlayToggle::from_slug("MergeTornado"), None);
    assert!(
        !OverlayToggle::ALL
            .iter()
            .any(|t| matches!(t.slug().as_str(), "Tds" | "Couplets")),
        "the merged layers are not toggles of their own any more"
    );
    // A saved workspace naming only the debris layer reports nothing it cannot restore.
    let mut ws = crate::workspace::starters().remove(0);
    ws.overlays_on = vec!["Tds".into()];
    assert!(
        crate::workspace::problems(&ws)
            .iter()
            .all(|p| !p.contains("layers this version does not have")),
        "{:?}",
        crate::workspace::problems(&ws)
    );
}

#[test]
fn every_contour_kind_is_saved_and_read_back_by_its_token() {
    for k in ContourKind::ALL {
        match k.token() {
            Some(t) => assert_eq!(ContourKind::from_token(t), Some(k), "{k:?}"),
            None => assert_eq!(k, ContourKind::Off),
        }
    }
}

#[test]
fn goto_parses_every_form_it_arrives_in() {
    let g = parse_goto("KTLX,-97.3,35.3,9").unwrap();
    assert_eq!(
        (g.site.as_str(), g.lon, g.lat, g.zoom),
        ("KTLX", -97.3, 35.3, 9.0)
    );
    assert!(g.time.is_none() && g.moment.is_none() && g.tilt.is_none());
    // The URL form is the same string behind a scheme.
    assert_eq!(
        parse_goto("hookecho://goto/KTLX,-97.3,35.3,9")
            .unwrap()
            .site,
        "KTLX"
    );
    // AlertService writes a site-less notification link; that must keep working.
    assert_eq!(parse_goto(",-97.3,35.3,9").unwrap().site, "");
    // Archive links carry a time.
    let g = parse_goto("KTLX,-97.3,35.3,9,2013-05-20T20:00:00Z").unwrap();
    assert_eq!(g.time.unwrap().to_rfc3339(), "2013-05-20T20:00:00+00:00");
    // A bare site resolves from the registry.
    let g = parse_goto("ktlx").unwrap();
    assert_eq!(g.site, "KTLX");
    assert!(g.lon < -97.0 && g.lat > 35.0 && g.zoom == 8.0);
    // Product and tilt are sniffed by shape, in either order.
    for v in ["KTLX,-97.3,35.3,9,VEL,2", "KTLX,-97.3,35.3,9,2,VEL"] {
        let g = parse_goto(v).unwrap();
        assert_eq!((g.moment, g.tilt), (Some(Moment::Velocity), Some(2)), "{v}");
    }
    assert!(parse_goto("").is_none());
    assert!(parse_goto("garbage").is_none());
}

#[test]
fn goto_survives_a_percent_encoded_round_trip() {
    // What a chat client hands back after eating the link.
    let g = parse_goto("KTLX%2C-97.3%2C35.3%2C9%2C2013-05-20T20%3A00%3A00Z").unwrap();
    assert_eq!(g.site, "KTLX");
    assert_eq!(g.zoom, 9.0);
    assert_eq!(g.time.unwrap().to_rfc3339(), "2013-05-20T20:00:00+00:00");
    // Encoded spaces around the fields, and a stray `%` that is not an escape at all.
    assert_eq!(parse_goto("%20ktlx%20").unwrap().site, "KTLX");
    assert!(parse_goto("%GG").is_none());
}

#[test]
fn goto_link_round_trips() {
    let base = |moment, tilt, threshold| Goto {
        site: "KFWS".to_string(),
        lon: -97.3031,
        lat: 32.5731,
        zoom: 8.5,
        time: None,
        moment: Some(moment),
        tilt: Some(tilt),
        basemap: None,
        threshold,
        srv: false,
        gauges: Vec::new(),
        tropical: None,
    };
    let link = goto_link(&base(Moment::Reflectivity, 0, None));
    assert!(link.starts_with("hookecho://goto/KFWS,"), "{link}");
    let g = parse_goto(&link).unwrap();
    assert_eq!(g.site, "KFWS");
    assert!((g.lon - -97.3031).abs() < 1e-4 && (g.lat - 32.5731).abs() < 1e-4);
    assert_eq!(g.zoom, 8.5);
    // Reflectivity at the base tilt is the default, so it stays out of the link.
    assert!(!link.contains("dBZ"), "{link}");

    let link = goto_link(&base(Moment::Velocity, 3, None));
    let g = parse_goto(&link).unwrap();
    assert_eq!((g.moment, g.tilt), (Some(Moment::Velocity), Some(3)));
    // No threshold set means the link says nothing, leaving the recipient's own alone.
    assert!(!link.contains("thr:"), "{link}");

    // Issue #71: an embedded dashboard needs to deep-link a threshold, and the link the Copy
    // button produces has to come back as the threshold it was copied from.
    let link = goto_link(&base(Moment::Reflectivity, 0, Some(Some(25.0))));
    assert!(link.contains(",thr:25"), "{link}");
    assert_eq!(parse_goto(&link).unwrap().threshold, Some(Some(25.0)));
    // A view with the threshold switched off shares as "nothing to say", not as `thr:off` —
    // overriding the recipient's own setting with a default nobody chose.
    assert!(!goto_link(&base(Moment::Reflectivity, 0, Some(None))).contains("thr:"));
}

#[test]
fn goto_parses_a_threshold_by_shape() {
    // Order does not matter, and the field is sniffed by shape like every other extra.
    assert_eq!(
        parse_goto("KTLX,-97.3,35.3,8,thr:25,VEL")
            .unwrap()
            .threshold,
        Some(Some(25.0))
    );
    // Off is a deliberate instruction, distinct from saying nothing at all.
    assert_eq!(
        parse_goto("KTLX,-97.3,35.3,8,thr:off").unwrap().threshold,
        Some(None)
    );
    assert_eq!(parse_goto("KTLX,-97.3,35.3,8").unwrap().threshold, None);
    // Garbage is ignored, not applied as zero.
    assert_eq!(
        parse_goto("KTLX,-97.3,35.3,8,thr:loud").unwrap().threshold,
        None
    );
}

/// The draw tool must append into the stroke in flight and start a new one per drag, and Undo
/// must drop exactly one stroke — the whole contract of a scribble layer.
#[test]
fn draw_strokes_append_and_undo() {
    let red = DRAW_COLORS[0];
    let cyan = DRAW_COLORS[2];
    let mut strokes = Vec::new();
    draw_append(&mut strokes, [-97.0, 35.0], red, true);
    draw_append(&mut strokes, [-97.1, 35.1], red, false);
    // A repeated point (a still finger during a drag) adds nothing.
    draw_append(&mut strokes, [-97.1, 35.1], red, false);
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].points.len(), 2);

    draw_append(&mut strokes, [-98.0, 36.0], cyan, true);
    assert_eq!(strokes.len(), 2);
    assert_eq!(strokes[1].color, cyan);

    strokes.pop(); // Undo
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].color, red);
}
