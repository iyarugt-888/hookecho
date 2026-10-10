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

## ECMWF leads past F+144 (1008.md E3, 2026-10-10)

[ecmwf-leads.txt](ecmwf-leads.txt) records a probe of data.ecmwf.int's index files for the
2026-10-09 runs. The 00Z and 12Z runs publish six-hourly files to F+360 (F+366 absent), and the
06Z run stops at F+144. The 00Z F+360 index lists 184 messages. The model browser's lead table
now reaches F+360 for 00/12Z. A browsed ECMWF field under a GFS pane scrubs on the ECMWF's own
leads (`app::models::scrub_range`).

## RRFS v1.0 (1008.md E3, added 2026-10-10)

RRFS v1.0 (CONUS 3 km) is a model in the catalogue (`wxdata::hrrr::Model::Rrfs`) and in the
model browser (reflectivity, CAPE, SRH, updraft helicity), read from NOMADS
(`rrfs/v1.0/rrfs.YYYYMMDD/HH/rrfs.tHHz.2dfld.3km.fNNN.conus.grib2`).

- **Cycles:** only the 00/06/12/18Z cycles write hourly CONUS files, to F+84. The cycles between
  write sub-hourly files only, to F+18. So the model cycles every 6 hours here.
- **Retention:** NOMADS keeps two days, so the run list holds 8 runs and there is no archive.
- **Posting:** the 2026-10-10 12Z run posted F+000 at 13:48 UTC and F+084 at 15:23; the
  catalogue uses 110 min as the typical latency.
- **Fields:** the `.idx` spells reflectivity, MSLP (MSLET) and the 5000–2000 m updraft helicity
  as the NAM family does. All 8 core messages are present at every hour checked (F+005, 007,
  013, 047, 084). It also carries ASNOW and 8 m MASSDEN, whose first message is particulate
  organic matter, the HRRR's smoke. Both are mapped, because the contract test's negative check
  requires it. The browser offers neither for RRFS, as for every model but the HRRR.

**Live checks, 2026-10-10:**

- `the_catalogue_matches_what_the_feeds_publish`: RRFS 12Z F+1 agreed with the table in both
  directions, as did the other eight models.
- `reflectivity_decodes_for_every_model_that_publishes_it`: RRFS F+6 regridded to 1830×787,
  1,088,905 cells.

The log is `target/parity-review/m5.3/rrfs-live.log`.

**Render:** `hookecho --headless-hrrr refc 6 <out.png> rrfs` on the RTX 2060. The 12Z run, valid
18Z, peaks at 68.6 dBZ: [rrfs-f06.png](rrfs-f06.png) (SHA-256 `50274d9b74699f469777d782282bc70f84d7bab81311726790de733f3e519d92`).

**Not established:**

- RRFS in the Model fields browser. HiresW is not there either.
- The browser build: NOMADS is not proxied (see [web-walk](../web-walk/README.md)).
- HREF's other ensemble products: mean, spread, probabilities and the rest; the PMM is below.

## HREF probability-matched mean (1008.md E3, added 2026-10-10)

The HREF's probability-matched mean is a model in the catalogue (`wxdata::hrrr::Model::HrefPmm`,
`Ensemble::PostProcessed`, like the NBM) and in the model browser (reflectivity, updraft
helicity). It is read from NOMADS (`href/prod/href.YYYYMMDD/ensprod/href.tHHz.conus.pmmn.fNN.grib2`).

- **Cycles:** 00/06/12/18Z, F+01 to F+48. There is no F+00.
- **Posting:** about three hours after the cycle. The 2026-10-10 12Z products posted F+01 at
  14:59 UTC and F+48 at 15:16.
- **Retention:** two days, so 8 runs.
- **Contents:** the `pmmn` file holds REFD, MAXREF, REFC, RETOP, MXUPHL, surface height and precipitation. The
  HREF's CAPE and helicity are in the separate ensemble-mean file, a different statistic, so they
  are not mapped to this model.
- **Grid:** the products are on NCEP grid 227 (Lambert, 1473×1025, 5.079 km), read off the GRIB
  grid definition, not on the members' 3 km. The first render at the 3 km regrid spacing showed
  a hatch of empty cells, so the model regrids at 0.06° and is labelled 5 km.

**Live checks, 2026-10-10:** the contract test agreed for all ten models. HREF PMM F+6
reflectivity regridded to 1725×819, with 481,816 cells filled; the grid extends well past CONUS
and is empty there. The log is `target/parity-review/m5.3/href-live.log`.

**Render:** `hookecho --headless-hrrr refc 6 <out.png> href-pmm`, 12Z valid 18Z, peaking at
56.6 dBZ. It shows the same systems as the RRFS render, smoother as a mean should be:
[href-pmm-f06.png](href-pmm-f06.png) (SHA-256 `d35f0f55ca9d0d587c299991634fddce9b57168db616d5cb421b53c3b0d598ee`).
