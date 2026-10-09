# M5.3 HiresW (1008.md E3)

HiresW ARW and FV3 (CONUS 2.5 km, 00/12Z to 48 h) are models in the catalogue
(`wxdata::hrrr::Model::{HireswArw, HireswFv3}`, `wxdata::model` definitions) and in the model
browser (reflectivity, CAPE, updraft helicity). They are read from NCEP's NOMADS, because neither
is on NOAA's AWS buckets; NOMADS keeps about two days, so the run list holds four runs and there
is no archive.

Every catalogue mapping was read off real `.idx` files and is held by the network contract test
`the_catalogue_matches_what_the_feeds_publish` (passed 2026-10-09 against the 2026-10-08 12Z
runs for every model, HiresW included). HiresW publishes its 2 m temperature and dewpoint only
every third hour (f00, f03, f06…), so those are not offered from it; it carries 0–1 km helicity
but not 0–3 km.

## Reviewed renders

`hookecho --headless-hrrr refc 6 <out.png> hiresw-arw` (and `hiresw-fv3`), RTX 2060, from the
2026-10-08 12Z runs, valid 18Z.

| File | SHA-256 |
| --- | --- |
| [hiresw-arw-f06.png](hiresw-arw-f06.png) | `1ce49f15fada18de4c4914913dfd9dcbf5ab799f7dcae3359ea80221086728dd` |
| [hiresw-fv3-f06.png](hiresw-fv3-f06.png) | `b2c7f7fc2282c9b0dca27dd6f00a8a3aa131ec35e50e384ad0aab041c341f3dd` |

## HREF and RRFS (evaluated 2026-10-09, not added)

- Neither HREF nor HiresW is on AWS (`noaa-href-pds`, `noaa-hiresw-pds` do not exist).
- HREF: NOMADS `href/prod/href.YYYYMMDD/ensprod/`, CONUS and Hawaii ensemble products (`mean`,
  `avrg`, `pmmn`, `lpmm`, `prob`, `sprd`, `eas`, `ffri`). Ensemble statistics, not a member, so
  they belong with M5.3's ensemble work rather than as a deterministic model.
- RRFS: `noaa-rrfs-pds` on AWS holds retrospective and prototype output only (`rrfs_a/` empty);
  NOMADS serves `rrfs/v1.0/rrfs.YYYYMMDD/HH/` hourly, with 3 km CONUS 2D fields
  (`rrfs.tHHz.2dfld.3km.fNNN.conus.grib2`, plus sub-hourly) — the next model to add.
