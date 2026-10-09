# M4.2 shapefile bundle picker (1008.md D1)

Importing a `.zip` that holds several shapefiles opens **Import shapefile bundle**: each dataset
with its shape count and anything incomplete (a missing `.dbf` or `.prj`), ticked to import. Each
ticked dataset becomes its own layer, whose source names the dataset inside the bundle
(`bundle.zip#folder/roads.shp`) so it reopens as itself; a bundle that no longer holds it says so
by name. **All as one layer** keeps the earlier behaviour. In a browser each dataset is kept as its
own GeoJSON, as other browser imports are. A zip with one shapefile imports directly.

Tested on a bundle built from two pinned real datasets (Oklahoma's 77 counties in NAD83 and in UTM
14N, `crates/wxdata/tests/data/gis/`): both listed, the UTM one loaded by name (case-insensitively)
and reopened from a `bundle.zip#utm/...` source with all 77 counties, and a missing dataset refused
by name (`gis_import::bundle_tests::a_bundle_dataset_loads_as_itself`).

| File | What | SHA-256 |
| --- | --- | --- |
| [bundle-picker.png](bundle-picker.png) | The picker for three datasets, one without its `.dbf` (RTX 2060, `gpu_bundle_picker_snapshot`) | `6388b930e0e1cdc6df387fb25065491bf02cd3e20450b2b7d36f0e07923a11f8` |

Not established: an interactive desktop import of a real multi-dataset bundle, Android's picker
and app-managed storage, and a real emergency-asset export fixture.
