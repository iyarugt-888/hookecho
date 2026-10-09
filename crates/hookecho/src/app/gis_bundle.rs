//! The dataset picker for zipped shapefile bundles (ROADMAP_PARITY M4.2, 1008.md D1): a `.zip`
//! holding several shapefiles opens a list of them — name, shape count and anything incomplete
//! (a missing `.dbf` or `.prj`) — to tick, and each one ticked becomes a layer of its own, its
//! source naming the dataset inside the bundle (`bundle.zip#roads.shp`) so it reopens as itself.
//! "All as one layer" keeps the earlier behaviour. A bundle with one shapefile imports directly.

use super::*;
use wxdata::shapefile::Dataset;

pub(crate) struct BundlePick {
    /// The zip's name, as picked.
    file: String,
    /// Where it is, for a layer's source; `None` in a browser, which keeps each dataset's
    /// content instead.
    path: Option<String>,
    datasets: Vec<Dataset>,
    chosen: Vec<bool>,
}

/// The layer source for dataset `name` of a bundle: `path#name` where the bundle has a path,
/// else the name a browser keeps its content under.
fn dataset_source(path: Option<&str>, file: &str, name: &str) -> String {
    match path {
        Some(p) => format!("{p}#{name}"),
        None => {
            let stem = |s: &str| {
                let s = s.rsplit('/').next().unwrap_or(s);
                s.rsplit_once('.').map_or(s, |(a, _)| a).to_string()
            };
            format!("{}-{}.geojson", stem(file), stem(name))
        }
    }
}

/// The picker's contents: the bundle's datasets to tick, and `Some(together)` when a button
/// was pressed (`false`: the ticked ones as separate layers; `true`: all as one).
fn picker_body(ui: &mut egui::Ui, pick: &mut BundlePick) -> Option<bool> {
    let mut action = None;
    ui.label(format!(
        "{} holds {} shapefiles. Each one ticked becomes its own layer.",
        pick.file,
        pick.datasets.len()
    ));
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for (d, on) in pick.datasets.iter().zip(pick.chosen.iter_mut()) {
                ui.checkbox(on, format!("{} \u{b7} {} shapes", d.name, d.features.len()));
                if !d.notes.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(d.notes.join("; ")).weak()).wrap());
                }
            }
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let any = pick.chosen.iter().any(|c| *c);
        if ui
            .add_enabled(any, egui::Button::new("Import ticked"))
            .clicked()
        {
            action = Some(false);
        }
        if ui
            .button("All as one layer")
            .on_hover_text("Every shapefile in the zip together, as before")
            .clicked()
        {
            action = Some(true);
        }
    });
    action
}

impl HookEchoApp {
    /// Open the picker when `import` is a zip of several shapefiles; `false` (and nothing held)
    /// for anything else, which imports as before.
    pub(crate) fn offer_bundle(&mut self, import: &crate::dialog::Import) -> bool {
        let name = import.name();
        if !crate::gis_import::is_zip(&name) {
            return false;
        }
        let bytes = match &import.bytes {
            Some(b) => b.clone(),
            None => match std::fs::read(&import.path) {
                Ok(b) => b,
                Err(_) => return false,
            },
        };
        let Ok(datasets) = crate::gis_import::zip_datasets(&bytes) else {
            // Let the ordinary import report what is wrong with it.
            return false;
        };
        if datasets.len() < 2 {
            return false;
        }
        let chosen = vec![true; datasets.len()];
        self.gis_bundle = Some(BundlePick {
            file: name,
            path: import
                .bytes
                .is_none()
                .then(|| import.path.to_string_lossy().into_owned()),
            datasets,
            chosen,
        });
        true
    }

    /// The picker window, while a bundle waits.
    pub(crate) fn bundle_picker(&mut self, ctx: &egui::Context) {
        let Some(pick) = self.gis_bundle.as_mut() else {
            return;
        };
        let mut open = true;
        let mut action = None;
        egui::Window::new("Import shapefile bundle")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(380.0)
            .show(ctx, |ui| action = picker_body(ui, pick));
        if !open {
            self.gis_bundle = None;
            return;
        }
        if let Some(together) = action {
            if let Some(pick) = self.gis_bundle.take() {
                self.import_bundle(pick, together);
            }
        }
    }

    fn import_bundle(&mut self, pick: BundlePick, together: bool) {
        let BundlePick {
            file,
            path,
            datasets,
            chosen,
        } = pick;
        let mut layers: Vec<(String, Vec<wxdata::gis::GisFeature>)> = Vec::new();
        if together {
            let features: Vec<_> = datasets.into_iter().flat_map(|d| d.features).collect();
            let source = path.clone().unwrap_or_else(|| {
                let stem = file.rsplit_once('.').map_or(file.as_str(), |(a, _)| a);
                format!("{stem}.geojson")
            });
            layers.push((source, features));
        } else {
            for (d, on) in datasets.into_iter().zip(chosen) {
                if on {
                    layers.push((dataset_source(path.as_deref(), &file, &d.name), d.features));
                }
            }
        }
        let mut shapes = 0;
        let mut last = None;
        let count = layers.len();
        for (source, features) in layers {
            if path.is_none() {
                // A browser has no path to reopen: keep the content, as other imports do.
                self.settings
                    .web_files
                    .insert(source.clone(), wxdata::gis::to_geojson(&features));
            }
            let id = self.add_gis_import(source, features);
            shapes += self.gis_loaded(id).map_or(0, |l| l.len());
            last = Some(id);
        }
        if count == 1 {
            self.zoom_to_gis(last);
        } else {
            self.zoom_to_gis(None);
        }
        self.toast(
            ToastKind::Info,
            format!(
                "Imported {shapes} shapes from {file} as {count} layer{}",
                if count == 1 { "" } else { "s" }
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picker for a bundle of three datasets, one without its .dbf, for review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the shapefile bundle picker"]
    fn gpu_bundle_picker_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the picker");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m4.2");
        std::fs::create_dir_all(&destination).unwrap();
        let dataset = |name: &str, n: usize, notes: &[&str]| Dataset {
            name: name.into(),
            features: (0..n)
                .map(|i| wxdata::gis::GisFeature {
                    geometry: wxdata::gis::Geometry::Point([-97.0 + i as f64 * 0.01, 35.0]),
                    properties: Default::default(),
                })
                .collect(),
            notes: notes.iter().map(|s| s.to_string()).collect(),
        };
        let mut pick = BundlePick {
            file: "county_assets.zip".into(),
            path: Some("C:/gis/county_assets.zip".into()),
            datasets: vec![
                dataset("assets/hospitals.shp", 14, &[]),
                dataset("assets/fire_stations.shp", 31, &[]),
                dataset("assets/sirens.shp", 58, &["no .dbf, so no attributes"]),
            ],
            chosen: vec![true, true, false],
        };
        gpu.save(&destination.join("bundle-picker.png"), 420, 240, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(400.0);
                ui.strong("Import shapefile bundle");
                picker_body(ui, &mut pick);
            });
        })
        .unwrap();
    }

    #[test]
    fn each_dataset_reopens_as_itself() {
        assert_eq!(
            dataset_source(Some("C:/gis/county.zip"), "county.zip", "data/roads.shp"),
            "C:/gis/county.zip#data/roads.shp"
        );
        let (zip, ds) = crate::gis_import::split_dataset("C:/gis/county.zip#data/roads.shp");
        assert_eq!((zip, ds), ("C:/gis/county.zip", Some("data/roads.shp")));
        assert_eq!(
            dataset_source(None, "county.zip", "data/roads.shp"),
            "county-roads.geojson"
        );
        // Not a bundle dataset: left as it is.
        assert_eq!(
            crate::gis_import::split_dataset("C:/gis/notes#1.geojson"),
            ("C:/gis/notes#1.geojson", None)
        );
    }
}
