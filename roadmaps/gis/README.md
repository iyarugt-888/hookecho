# GIS — operator interoperability

Parent M4.1–M4.4; ROADMAP_NEW I; ROADMAP_2 §5; newsuggestions S1–S3/S7/S8.
[Evidence](../../docs/certification/m4.2/).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| G1 | Certification-only | Import a real projected multi-dataset shapefile bundle, inspect/filter/style, save case/workspace/scene, restore elsewhere, export and read in QGIS/GDAL. Compare coordinates, holes, multipart geometry, encodings and attributes independently. |
| G2 | Partially implemented | Close dense-layer label priority/collision and operational attribute/symbol workflow gaps found by G1; reuse the collection, style and filter owners. |
| G3 | Certification-only | Physical Android SAF/app-managed storage and browser portability, removed/missing IDs and malformed/oversized bundle refusal. |
| G4 | Still needed | WFS/FeatureServer or broader projections only for a demonstrated customer workflow; deferred. |

Collections/groups, GeoJSON points/lines/polygons, KML/KMZ, shapefile sidecars,
supported projections, style rules, portable manifests and threshold outlines already
exist. “Add GIS import/export” is superseded; external interoperability is not yet proved.
