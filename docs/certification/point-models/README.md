# Every model at the tapped point, for any product (2026-10-12)

The forecast window's model graph read one of four global models (GFS, ECMWF, GEFS, GDPS) for one
of five fields, one line at a time. "Models at this point" (`app/point_models.rs`) graphs up to six
of fifteen models on one time axis: HRRR, RAP, NAM 3 km and 12 km, HiresW ARW and FV3, RRFS, HREF
mean and PMM, NBM, the HRRR's pressure-level file, GFS, ECMWF, the GEFS mean and the GDPS. The
product is one of 15 common ones (2 m temperature, dew point and humidity, 10 m wind, gust,
sea-level pressure, precipitation since the run began, surface CAPE, composite reflectivity, total
cloud, visibility, precipitable water, 850 hPa temperature, 500 hPa height, snow depth) or any
field with vetted units picked from one model's own file inventory ("Any field…").

**How a line is made.** Each model's line is one run: its inventory is read at lead 12, which pins
the newest run with that lead posted. A short HRRR or RAP run that ends before the span gives way
to the newest long one (`a_short_run_reaches_back_to_the_newest_long_one`). The product is matched
in that model's own inventory by quantity, level and timing kind, so a model without it says "not
in this model's file" rather than standing in another field
(`a_product_is_matched_in_each_inventory_or_not_at_all`). Lead 12, not 6, because the GEFS's
precipitation is a total since the run began only at lead 6 and 6-hour buckets after it. Leads
land on the same valid hours in every model (hourly over 24 and 48 hours, 3-hourly over 5 days,
6-hourly over 10), only on files the model publishes (the NAM 12 km past F+36, the NBM, the ECMWF
and GEFS past their changes of step; `leads_line_up_on_valid_time_and_published_files`). A lead
the run has not posted is a gap in its line. Winds are speed in knots from both components.
Temperatures follow the temperature-unit setting. Leads are read four at a time per model, two
from NOMADS, which is also read every third hour at most; the line fills in as they arrive, and a
finished line is reused for 15 minutes. The ticked models and a common product are saved in
settings (`a_saved_choice_comes_back`).

**The graph** (`ui::forecast_window::paint_lines`) has a line per model in a fixed colour, "now"
marked, 6-hourly ticks up to three days and local midnights beyond. Pointing at it (or touching
it) reads every line at that hour. Under the graph, each model's run, its range over the span, how
many leads are missing, or why it has no line.

**Also fixed on the way.**

- The GEFS mean's inventory is readable (`wxdata::global`, the `pgrb2a` half-degree file, 3-hourly
  to F+240 then 6-hourly).
- The proxy allowlists (`serve.rs`, `web/_worker.js/proxy-core.js`) gain `noaa-gefs-pds`. The
  browser build could not reach the GEFS at all, so the ensemble plume failed there with 403; it
  draws now (bottom of the first capture).
- The inventory parser reads NCEP's whole-day timings ("0-1 day acc fcst", "2 day fcst"). At F+24
  and F+48 a total since the run began was unread, so a precipitation series had a hole every 24
  hours (`whole_days_read_as_hours`).

**What the browser build cannot read.** The RAP's `awp130` files and the GDPS's Datamart files are
JPEG 2000 packed (section 5 template 40, read off the files), and the web build leaves that codec
out. NOMADS (HiresW, RRFS, HREF) sends no CORS headers and limits requests per address, so it is
not put behind the shared proxy. In the browser those six models say "desktop and Android only"
and why. The GDPS host is not added to the allowlists for the same reason.

**Live, desktop test build** (`every_model_at_a_point_live`, Oklahoma City, 48 hours every 6 h):
fourteen of the fifteen gave 2 m temperature; HREF PMM has no 2 m temperature or wind and said
so, and HREF mean and the NBM have no 10 m wind pair. For precipitation since the run began, the
HRRR (both files), RAP, HiresW ARW and FV3, RRFS, GFS, ECMWF and GDPS had it; the NAM 3 km and 12 km, the NBM, the
GEFS mean and both HREF products said "not in this model's file". That run was before the
whole-day fix, and showed its holes at F+24 and F+48 (`target/parity-review/pointmodels-live.txt`).

**Browser build on WebGPU, one machine**, built from the working tree at `6a5064ea` with this
change and another session's uncommitted work, an 800 by 625 pane, Oklahoma City.

| Capture | What it shows |
| --- | --- |
| `temperature-six-models-browser.jpg` | 2 m temperature over 48 hours from the HRRR 18Z, NAM 3 km 18Z, GFS 12Z, ECMWF 12Z and GEFS mean 12Z runs, read at Tue 6 PM CDT: 83, 83, 84, 84 and 85 °F. The GDPS: "the browser build cannot decode its JPEG 2000 files". Below, the GEFS plume. Taken before the RAP and NOMADS labels and the whole-day fix, which change nothing shown here. |
| `precipitation-and-browser-limits-browser.jpg` | Precipitation since the run began over 48 hours, read at Wed 10 AM: HRRR 3.0, GFS 12.8, ECMWF 14.1 mm, with no leads missing. The RAP and HiresW FV3 say "desktop and Android only" and why. |
| `any-field-helicity-browser.jpg` | "Any field…" on the HRRR's 21Z inventory (96 fields with vetted units), searched for "helicity", 0–3000 m storm-relative helicity picked: the HRRR 9–250 and GFS 26–238 m²/s²; the ECMWF "not in this model's file". |

| File | SHA-256 |
| --- | --- |
| temperature-six-models-browser.jpg | `4804f411ae46a242abe6c127c7a9fe33701b358b7a9285626ef8726e1dab8e37` |
| precipitation-and-browser-limits-browser.jpg | `f8aa2829f1a9c5292869a90b03064263317463d72e57df07edf62cc724769e9d` |
| any-field-helicity-browser.jpg | `2bab805699f8410a011402548d7a629ad398880236ac8ef0759cad7084cf200f` |

Not established: the desktop and Android apps' windows (the desktop path is the live test, not
the window), a phone layout, the 10-day span in the browser, a point outside the regional grids,
and how long a line takes on a slow connection (in the browser six models at once took about a
minute for 48 hourly leads, the decoding sharing the page's one thread).
