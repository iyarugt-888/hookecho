//! Settings bundles picked and applied. Moved out of `app.rs` unchanged.
use super::*;

impl HookEchoApp {
    /// Import a settings bundle (rfd open dialog). The next-frame dirty-diff reloads palettes
    /// and persists, and the UI (theme, layers, markers…) updates live from the new settings.
    pub(crate) fn import_settings_bundle(&mut self) {
        crate::dialog::request_open(crate::dialog::ImportKind::SettingsBundle, "");
    }

    /// Apply a settings bundle the user picked.
    pub(crate) fn apply_settings_bundle(&mut self, import: &crate::dialog::Import) {
        match import
            .text()
            .and_then(|s| crate::settings::Settings::import_bundle(&s))
        {
            Ok(mut settings) => {
                // A bundle written before M4.1 carries the one layer the old way.
                settings.migrate_imported_gis();
                self.settings = settings;
                // The layers are reloaded from the bundle's own list (`sync_gis_layers`).
                self.gis.clear();
                self.toast(ToastKind::Success, "Settings imported");
            }
            Err(e) => {
                log::warn!("settings import failed: {e}");
                self.toast(ToastKind::Error, format!("Settings import failed: {e}"));
            }
        }
    }
}
