//! theme_plan.md §4: Analyst Mode's live log — a filtered, auto-following view of
//! [`crate::devlog`]'s capture buffer, showing the radar/provider detail an analyst wants that
//! never surfaces at the app's normal `info` log level: live-sweep chunk arrival (tilt/VCP
//! progress, decode time, retries) and, when a relay is configured, provider health and failover
//! transitions from this session's own B6 work.
//!
//! Adds no new instrumentation — every line here was already a `log::debug!` call site before
//! this window existed; `crate::devlog::set_analyst_mode` is what makes those calls actually reach
//! the capture buffer at all (see that function's own doc comment for why `debug!` is normally a
//! no-op before a `Record` is ever constructed), and this window is only what reads it back out.
//!
//! Gated entirely on `Settings.analyst_mode` — `show` returns immediately when it's off, so this
//! costs nothing (not even the buffer read) while the feature isn't in use.

/// Every log target this window shows. A plain prefix list, not a registry — the targets that
/// matter to an analyst are exactly the ones named in theme_plan.md §4, not something meant to
/// grow into a general-purpose log category system.
const TARGET_PREFIXES: &[&str] = &[
    "hookecho::live_sweep",
    "hookecho::provider_health",
    "hookecho::failover_arbiter",
    "hookecho::radar_provider_manager",
];

/// How many recent matching lines to keep on screen. Generous enough to scroll back through a
/// whole volume's worth of live-sweep chunks, small enough that formatting it every frame stays
/// cheap.
const MAX_LINES: usize = 400;

pub(crate) fn show(
    ctx: &egui::Context,
    settings: &mut crate::settings::Settings,
    drawer: &mut crate::ui::drawer::Drawer,
) {
    if !settings.analyst_mode {
        return;
    }
    let mut keep = true;
    let Some(window) = drawer.page_sized(
        ctx,
        "Analyst log",
        &mut keep,
        false,
        420.0,
        egui::Window::new("Analyst log"),
    ) else {
        // Closing the window (its own X, not the Settings checkbox) is a second way to turn
        // Analyst Mode off — keep both paths consistent, including the log-level restore, rather
        // than leaving the level raised with the checkbox now silently out of sync.
        settings.analyst_mode = keep;
        crate::devlog::set_analyst_mode(keep);
        return;
    };
    window.show(ctx, |ui| body(ui, 360.0));
    // Mirrors the early-return branch above: the window's own close button can flip `keep` to
    // false during `show()`, and that has to turn Analyst Mode off the same consistent way the
    // Settings checkbox does (including restoring the log level), not just update the flag.
    if keep != settings.analyst_mode {
        settings.analyst_mode = keep;
        crate::devlog::set_analyst_mode(keep);
    }
}

/// The log itself: a line saying what it holds, then the newest matching lines, following the
/// bottom as they arrive. Drawn by the floating window and by the workstation's Analyst log tab.
pub(crate) fn body(ui: &mut egui::Ui, max_height: f32) {
    let entries = crate::devlog::recent(MAX_LINES, TARGET_PREFIXES);
    ui.weak(format!(
        "Live-sweep, provider-health and failover detail — {} line{} buffered.",
        entries.len(),
        if entries.len() == 1 { "" } else { "s" }
    ));
    ui.separator();
    if entries.is_empty() {
        ui.weak("Nothing yet — follow a live NEXRAD site to see sweep-by-sweep detail here.");
        return;
    }
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .max_height(max_height)
        .show(ui, |ui| {
            for e in &entries {
                let color = level_color(&e.level);
                let ts = chrono::DateTime::from_timestamp_millis(e.ts_ms)
                    .map(|d| d.format("%H:%M:%S%.3f").to_string())
                    .unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(ts)
                            .size(10.0)
                            .color(egui::Color32::from_gray(150))
                            .monospace(),
                    );
                    ui.label(
                        egui::RichText::new(short_target(&e.target))
                            .size(10.0)
                            .color(egui::Color32::from_gray(180))
                            .monospace(),
                    );
                    ui.label(
                        egui::RichText::new(&e.message)
                            .size(11.0)
                            .color(color)
                            .monospace(),
                    );
                });
            }
        });
}

fn level_color(level: &str) -> egui::Color32 {
    match level {
        "ERROR" => egui::Color32::from_rgb(230, 100, 100),
        "WARN" => egui::Color32::from_rgb(230, 180, 90),
        "DEBUG" | "TRACE" => egui::Color32::from_gray(160),
        _ => egui::Color32::from_gray(220),
    }
}

/// Drop the common `hookecho::` prefix every target here shares — it's implied by the window's
/// own title, and every extra character is width the actual message doesn't get.
fn short_target(target: &str) -> &str {
    target.strip_prefix("hookecho::").unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_target_drops_the_shared_crate_prefix() {
        assert_eq!(short_target("hookecho::live_sweep"), "live_sweep");
        assert_eq!(short_target("wxdata::tds"), "wxdata::tds");
    }

    #[test]
    fn level_color_gives_errors_and_warnings_distinct_colors() {
        assert_ne!(level_color("ERROR"), level_color("WARN"));
        assert_ne!(level_color("ERROR"), level_color("INFO"));
        assert_ne!(level_color("DEBUG"), level_color("INFO"));
    }
}
