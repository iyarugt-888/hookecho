//! Where each pane goes: the grid for a pane count, and the arranged layouts (focus, rows,
//! columns). Moved out of `app.rs` unchanged (ROADMAP_2 §7).

pub(crate) fn pane_rects(r: egui::Rect, n: usize) -> Vec<egui::Rect> {
    let gap = 2.0;
    match n {
        0 | 1 => vec![r],
        2 => {
            // Stack top/bottom when the viewport is taller than wide (portrait phone), else split
            // left/right (wide desktop). Gives the horizontal split the user wants on mobile.
            if r.height() >= r.width() {
                let h = (r.height() - gap) / 2.0;
                vec![
                    egui::Rect::from_min_size(r.min, egui::vec2(r.width(), h)),
                    egui::Rect::from_min_size(
                        egui::pos2(r.min.x, r.min.y + h + gap),
                        egui::vec2(r.width(), h),
                    ),
                ]
            } else {
                let w = (r.width() - gap) / 2.0;
                vec![
                    egui::Rect::from_min_size(r.min, egui::vec2(w, r.height())),
                    egui::Rect::from_min_size(
                        egui::pos2(r.min.x + w + gap, r.min.y),
                        egui::vec2(w, r.height()),
                    ),
                ]
            }
        }
        3 => {
            // ROADMAP_NEW J1's "3 pane": the same adaptive orientation as the 2-pane split above
            // (three rows in portrait, three columns in landscape) rather than falling through to
            // the 2x2 grid below truncated to three cells, which used to leave one quadrant of
            // screen permanently blank instead of splitting the space three ways.
            if r.height() >= r.width() {
                let h = (r.height() - gap * 2.0) / 3.0;
                (0..3)
                    .map(|i| {
                        egui::Rect::from_min_size(
                            egui::pos2(r.min.x, r.min.y + (h + gap) * i as f32),
                            egui::vec2(r.width(), h),
                        )
                    })
                    .collect()
            } else {
                let w = (r.width() - gap * 2.0) / 3.0;
                (0..3)
                    .map(|i| {
                        egui::Rect::from_min_size(
                            egui::pos2(r.min.x + (w + gap) * i as f32, r.min.y),
                            egui::vec2(w, r.height()),
                        )
                    })
                    .collect()
            }
        }
        // No UI offers exactly 5 panes (the palette jumps 4 -> 6, matching AWIPS convention), but
        // `apply_workspace` calls `set_pane_count(ws.panes.len())` with no bound of its own, so a
        // workspace file from another source naming exactly 5 panes can still reach here. Reusing
        // 5 of the 6-pane grid's own cells keeps every pane tiled and gap-free instead of falling
        // through to the 2x2 fallback below, which would silently drop a view with no rect at all
        // now that `set_pane_count`'s ceiling is past 4.
        5 => pane_rects(r, 6).into_iter().take(5).collect(),
        6 => {
            // ROADMAP_NEW J1's "6 pane": same adaptive-orientation convention as 2/3-pane above —
            // a 3x2 grid (3 columns, 2 rows) in landscape, transposed to 2x3 (2 columns, 3 rows)
            // in portrait, rather than reusing the 2x2 fallback below (which only ever has 4
            // cells to truncate, so it could never reach 6 in the first place).
            let (cols, rows) = if r.height() >= r.width() {
                (2, 3)
            } else {
                (3, 2)
            };
            let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
            let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
            let mut v = Vec::with_capacity(6);
            for row in 0..rows {
                for col in 0..cols {
                    let x = r.min.x + (w + gap) * col as f32;
                    let y = r.min.y + (h + gap) * row as f32;
                    v.push(egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(w, h),
                    ));
                }
            }
            v
        }
        7 | 8 => pane_rects(r, 9).into_iter().take(n).collect(),
        9 => {
            // ROADMAP_NEW J1's desktop/web analyst wall: a stable 3x3 grid in either orientation.
            // Seven- and eight-pane workspace imports reuse a prefix of these cells above, so no
            // accepted pane count can silently lose views to the old four-cell fallback.
            let cols = 3;
            let rows = 3;
            let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
            let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
            let mut v = Vec::with_capacity(9);
            for row in 0..rows {
                for col in 0..cols {
                    let x = r.min.x + (w + gap) * col as f32;
                    let y = r.min.y + (h + gap) * row as f32;
                    v.push(egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(w, h),
                    ));
                }
            }
            v
        }
        4 => {
            let w = (r.width() - gap) / 2.0;
            let h = (r.height() - gap) / 2.0;
            let mut v = Vec::new();
            for row in 0..2 {
                for col in 0..2 {
                    let x = r.min.x + (w + gap) * col as f32;
                    let y = r.min.y + (h + gap) * row as f32;
                    v.push(egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(w, h),
                    ));
                }
            }
            v
        }
        _ => pane_rects(r, crate::view::MAX_PANES),
    }
}

/// Apply the saved workspace arrangement on top of the pane count. The focus layout deliberately
/// keeps pane 0 first: active-pane selection, keyboard pane switching and workspace snapshots all
/// already use stable vector order, so making the primary pane a geometric concern avoids a
/// second, drifting notion of pane identity.
pub(crate) fn arranged_pane_rects(
    r: egui::Rect,
    n: usize,
    layout: crate::workspace::PaneLayout,
) -> Vec<egui::Rect> {
    if layout == crate::workspace::PaneLayout::Balanced || n <= 2 {
        return pane_rects(r, n);
    }

    let n = n.clamp(1, crate::view::MAX_PANES);
    let detail_count = n - 1;
    let gap = 2.0;
    // About five-eighths is enough for the primary pane to read as the workspace without turning
    // the detail rail into thumbnails. It also leaves pane 0 larger than every detail at all
    // supported counts (3..=9) and in either orientation.
    const PRIMARY_FRACTION: f32 = 0.62;
    let mut out = Vec::with_capacity(n);

    if r.width() >= r.height() {
        let usable = r.width() - gap;
        let primary_w = usable * PRIMARY_FRACTION;
        out.push(egui::Rect::from_min_size(
            r.min,
            egui::vec2(primary_w, r.height()),
        ));
        let rail = egui::Rect::from_min_max(egui::pos2(r.left() + primary_w + gap, r.top()), r.max);
        let cols = if detail_count <= 3 { 1 } else { 2 };
        let rows = detail_count.div_ceil(cols);
        let w = (rail.width() - gap * (cols - 1) as f32) / cols as f32;
        let h = (rail.height() - gap * (rows - 1) as f32) / rows as f32;
        for i in 0..detail_count {
            let row = i / cols;
            let col = i % cols;
            out.push(egui::Rect::from_min_size(
                egui::pos2(
                    rail.left() + (w + gap) * col as f32,
                    rail.top() + (h + gap) * row as f32,
                ),
                egui::vec2(w, h),
            ));
        }
    } else {
        let usable = r.height() - gap;
        let primary_h = usable * PRIMARY_FRACTION;
        out.push(egui::Rect::from_min_size(
            r.min,
            egui::vec2(r.width(), primary_h),
        ));
        let rail = egui::Rect::from_min_max(egui::pos2(r.left(), r.top() + primary_h + gap), r.max);
        let rows = if detail_count <= 3 { 1 } else { 2 };
        let cols = detail_count.div_ceil(rows);
        let w = (rail.width() - gap * (cols - 1) as f32) / cols as f32;
        let h = (rail.height() - gap * (rows - 1) as f32) / rows as f32;
        for i in 0..detail_count {
            let row = i / cols;
            let col = i % cols;
            out.push(egui::Rect::from_min_size(
                egui::pos2(
                    rail.left() + (w + gap) * col as f32,
                    rail.top() + (h + gap) * row as f32,
                ),
                egui::vec2(w, h),
            ));
        }
    }
    out
}

#[cfg(test)]
mod pane_rects_tests {
    use super::{arranged_pane_rects, pane_rects};
    use crate::workspace::PaneLayout;
    use egui::Rect;

    fn landscape() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 600.0))
    }
    fn portrait() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 1000.0))
    }

    #[test]
    fn one_pane_is_the_whole_rect() {
        let r = landscape();
        assert_eq!(pane_rects(r, 1), vec![r]);
    }

    #[test]
    fn three_panes_returns_exactly_three_rects() {
        // ROADMAP_NEW J1: this used to fall through to the 2x2 grid truncated to three cells,
        // silently leaving one quadrant of screen blank instead of splitting the space three ways.
        assert_eq!(pane_rects(landscape(), 3).len(), 3);
        assert_eq!(pane_rects(portrait(), 3).len(), 3);
    }

    #[test]
    fn three_panes_are_columns_in_landscape_and_rows_in_portrait() {
        let r = landscape();
        let cols = pane_rects(r, 3);
        // Columns: same height as the source rect, and each narrower than half of it (three
        // panes must be narrower than a two-pane split would make them).
        for c in &cols {
            assert!((c.height() - r.height()).abs() < 0.01, "{c:?}");
            assert!(c.width() < r.width() / 2.0, "{c:?}");
        }
        let p = portrait();
        let rows = pane_rects(p, 3);
        for row in &rows {
            assert!((row.width() - p.width()).abs() < 0.01, "{row:?}");
            assert!(row.height() < p.height() / 2.0, "{row:?}");
        }
    }

    #[test]
    fn three_panes_tile_the_source_rect_without_gaps_or_overlap() {
        // Each pane's leading edge should sit right after the previous one's trailing edge (plus
        // the fixed gap), and the whole strip should span start to finish.
        let r = landscape();
        let cols = pane_rects(r, 3);
        assert!((cols[0].min.x - r.min.x).abs() < 0.01);
        for w in cols.windows(2) {
            assert!(w[1].min.x > w[0].max.x, "{:?} vs {:?}", w[0], w[1]);
        }
        assert!((cols[2].max.x - r.max.x).abs() < 0.01);
    }

    #[test]
    fn four_panes_is_still_a_two_by_two_grid() {
        let rects = pane_rects(landscape(), 4);
        assert_eq!(rects.len(), 4);
        // Two distinct x positions and two distinct y positions, i.e. a real grid, not a strip.
        let xs: std::collections::BTreeSet<i64> =
            rects.iter().map(|r| (r.min.x * 100.0) as i64).collect();
        let ys: std::collections::BTreeSet<i64> =
            rects.iter().map(|r| (r.min.y * 100.0) as i64).collect();
        assert_eq!(xs.len(), 2, "{xs:?}");
        assert_eq!(ys.len(), 2, "{ys:?}");
    }

    #[test]
    fn five_panes_is_not_silently_truncated_to_four() {
        // No UI control offers 5 (the palette jumps 4 -> 6), but a foreign workspace file could
        // still ask for it via `set_pane_count`, which no longer clamps everything above 4 down
        // to 4 now that 6-pane exists — this must not regress into a dropped, unrendered view.
        assert_eq!(pane_rects(landscape(), 5).len(), 5);
        assert_eq!(pane_rects(portrait(), 5).len(), 5);
    }

    #[test]
    fn six_panes_returns_exactly_six_rects() {
        // ROADMAP_NEW J1's "6 pane": used to be unreachable at all (pane_rects only special-cased
        // up to 4, and the 2x2 fallback below can never produce more than 4 cells).
        assert_eq!(pane_rects(landscape(), 6).len(), 6);
        assert_eq!(pane_rects(portrait(), 6).len(), 6);
    }

    #[test]
    fn six_panes_is_a_three_by_two_grid_in_landscape_and_two_by_three_in_portrait() {
        let distinct_axes = |rects: &[Rect]| {
            let xs: std::collections::BTreeSet<i64> =
                rects.iter().map(|r| (r.min.x * 100.0) as i64).collect();
            let ys: std::collections::BTreeSet<i64> =
                rects.iter().map(|r| (r.min.y * 100.0) as i64).collect();
            (xs.len(), ys.len())
        };
        assert_eq!(distinct_axes(&pane_rects(landscape(), 6)), (3, 2));
        assert_eq!(distinct_axes(&pane_rects(portrait(), 6)), (2, 3));
    }

    #[test]
    fn six_panes_tile_the_source_rect_without_gaps_or_overlap() {
        let r = landscape();
        let rects = pane_rects(r, 6);
        // Every rect's bounds stay within the source rect (plus float slop)...
        for c in &rects {
            assert!(
                c.min.x >= r.min.x - 0.01 && c.max.x <= r.max.x + 0.01,
                "{c:?}"
            );
            assert!(
                c.min.y >= r.min.y - 0.01 && c.max.y <= r.max.y + 0.01,
                "{c:?}"
            );
        }
        // ...and the grid actually reaches every edge of the source rect, not a shrunken subset.
        let min_x = rects.iter().map(|c| c.min.x).fold(f32::MAX, f32::min);
        let max_x = rects.iter().map(|c| c.max.x).fold(f32::MIN, f32::max);
        let min_y = rects.iter().map(|c| c.min.y).fold(f32::MAX, f32::min);
        let max_y = rects.iter().map(|c| c.max.y).fold(f32::MIN, f32::max);
        assert!((min_x - r.min.x).abs() < 0.01);
        assert!((max_x - r.max.x).abs() < 0.01);
        assert!((min_y - r.min.y).abs() < 0.01);
        assert!((max_y - r.max.y).abs() < 0.01);
    }

    #[test]
    fn seven_and_eight_pane_workspace_imports_keep_every_view() {
        for n in [7, 8] {
            assert_eq!(pane_rects(landscape(), n).len(), n);
            assert_eq!(pane_rects(portrait(), n).len(), n);
        }
    }

    #[test]
    fn nine_panes_is_a_three_by_three_grid() {
        for source in [landscape(), portrait()] {
            let rects = pane_rects(source, 9);
            assert_eq!(rects.len(), 9);
            let xs: std::collections::BTreeSet<i64> =
                rects.iter().map(|r| (r.min.x * 100.0) as i64).collect();
            let ys: std::collections::BTreeSet<i64> =
                rects.iter().map(|r| (r.min.y * 100.0) as i64).collect();
            assert_eq!((xs.len(), ys.len()), (3, 3));
            assert!((rects.first().unwrap().min.x - source.min.x).abs() < 0.01);
            assert!((rects.first().unwrap().min.y - source.min.y).abs() < 0.01);
            assert!((rects.last().unwrap().max.x - source.max.x).abs() < 0.01);
            assert!((rects.last().unwrap().max.y - source.max.y).abs() < 0.01);
        }
    }

    #[test]
    fn focus_layout_makes_pane_one_the_large_primary() {
        for source in [landscape(), portrait()] {
            for n in [3, 4, 6, 9] {
                let rects = arranged_pane_rects(source, n, PaneLayout::Focus);
                assert_eq!(rects.len(), n);
                let primary_area = rects[0].area();
                assert!(
                    rects[1..].iter().all(|detail| primary_area > detail.area()),
                    "pane 1 was not primary for {n} panes: {rects:?}"
                );
            }
        }
    }

    #[test]
    fn focus_layout_is_a_left_rail_in_landscape_and_top_rail_in_portrait() {
        let wide = arranged_pane_rects(landscape(), 4, PaneLayout::Focus);
        assert!(wide[0].width() > wide[1].width());
        assert_eq!(wide[0].height(), landscape().height());
        assert!(wide[1..]
            .iter()
            .all(|detail| detail.left() > wide[0].right()));

        let tall = arranged_pane_rects(portrait(), 4, PaneLayout::Focus);
        assert!(tall[0].height() > tall[1].height());
        assert_eq!(tall[0].width(), portrait().width());
        assert!(tall[1..]
            .iter()
            .all(|detail| detail.top() > tall[0].bottom()));
    }

    #[test]
    fn focus_layout_keeps_every_supported_pane_inside_the_source_without_overlap() {
        for source in [landscape(), portrait()] {
            for n in 1..=crate::view::MAX_PANES {
                let rects = arranged_pane_rects(source, n, PaneLayout::Focus);
                assert_eq!(rects.len(), n);
                for (i, pane) in rects.iter().enumerate() {
                    assert!(
                        source.contains_rect(*pane),
                        "pane {i} escaped {source:?}: {pane:?}"
                    );
                    for other in &rects[i + 1..] {
                        assert!(
                            !pane.intersects(*other),
                            "focus panes overlap: {pane:?} vs {other:?}"
                        );
                    }
                }
            }
        }
    }
}
