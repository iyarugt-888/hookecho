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
- HREF's spread, neighbourhood probabilities and other products (the PMM and the mean are below).

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

## HREF ensemble mean (added 2026-10-10)

The HREF's ensemble mean (`Model::HrefMean`, `Ensemble::PostProcessed`, the same grid 227 and
schedule as the PMM) is in the catalogue and in the model browser, offering CAPE and SRH. The
mean file holds the environment: surface and 90–0 mb CAPE, 0–3 km helicity, MSLET, and 2 m
temperature and dewpoint, all at every hour checked (F+01, 05, 07, 13, 47, 48 of the 2026-10-10
12Z run). It holds no reflectivity, updraft helicity, 0–1 km helicity, snow or smoke; for
storms, the PMM is the ensemble's reflectivity. The contract test agreed for all eleven models
(`target/parity-review/m5.3/href-mean-live.log`).

`--headless-env` rendered at F+00, which the NBM and the HREF products do not publish. It now
asks for their first lead, F+01.

**Render:** `hookecho --headless-env sbcape <out.png> href-mean`, 12Z valid 13Z: surface CAPE
from 0 to 4,110 J/kg on a 1725×819 grid with 481,813 cells. See
[href-mean-sbcape.png](href-mean-sbcape.png) (SHA-256 `35e33edf10518f239c77aafc53692333b670c56d0b707225304cadf098b58c94`).

## The GEFS envelope on the Skew-T (1010.md V10, 2026-10-11)

Each GEFS member at a point comes from NOMADS's GRIB filter
(`wxdata::ensemble_sounding::member_url`): TMP and RH on the 7 `pgrb2a` and 12 `pgrb2b` levels
from 1000 to 200 hPa, in a one-degree box, a few kilobytes per file. That is two requests per
member instead of hundreds of range requests into the global files. All 31 members come from
one cycle, paced 650 ms apart.

| What | Result |
| --- | --- |
| One member live (`live_one_member_profile_from_nomads`) | GEFS 12Z f024 control: 19 levels at (-97.5, 35.5), the grid point nearest (-97.5, 35.4) |
| The whole ensemble live (`live_envelope_from_nomads`) | GEFS 12Z f033: 31 members, 19 levels, 64 s; at 1000 hPa T 29.1 to 33.7 °C, Td 12.7 to 21.3 °C; at 500 hPa T -6.0 to -4.8 °C (`target/parity-review/f8/live-envelope.log`) |
| The window, rendered (`gpu_sounding_envelope_review`) | HRRR 22Z f12 near Oklahoma City under GEFS 12Z f021: "31 members · run 2026-10-11 12:00Z f021 · valid 09:00Z, -1.0 h from this profile"; a narrow red temperature band along the HRRR trace, a wide green dew point band through the mid levels |

[sounding-gefs-envelope.png](sounding-gefs-envelope.png) (SHA-256
`33b42a6dfb12830fe655c48b2f8e64f0ec20f69670f53b14571e436137fed805`), rendered off screen by the
app's UI snapshot helper on this machine's GPU. It is not a capture of the running app.

Found while building it: a regular lat/lon GRIB message hands back its two axes, not a
coordinate per cell. Paired as cells, they picked (-97.0, 35.0) for a click at (-97.5, 35.4).
Expanded row by row, as `global` already did, they pick (-97.5, 35.5).

Limits:
- Temperature and dew point only: no member winds and no hodograph spread.
- The GEFS is half a degree and 3-hourly, so the valid time can be up to 1.5 h from the profile.
  The window states the offset.
- Desktop and Android only. NOMADS is not on the browser build's proxy (1010.md V12).
- The HRRR profile under the band starts at 1000 hPa even where that is below the ground. That
  is the model sounding's own pre-existing issue, filed separately. The band is cut at the
  profile's first level.
