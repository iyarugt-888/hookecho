# Models the browser cannot read (2026-10-12, 1010.md V12)

V12 is gated on the maintainer: if NOMADS may be proxied, add it to both allowlists; if not,
label those models "not available in the browser version" instead of showing a fetch error.
This is the second branch. Nothing was added to the proxy.

**Why these models.** Two separate reasons, both read off the sources:
- **JPEG 2000.** The RAP's `awp130` files and the GDPS's Datamart files are packed with GRIB2
  section 5 template 40 (read off a downloaded message of each), and the web build leaves the
  JPEG 2000 codec (openjpeg) out (`crates/wxdata/Cargo.toml`). HRRR, NAM, NBM, GFS, ECMWF and
  GEFS files are template 3 and decode in the browser.
- **NOMADS.** The HiresW, RRFS and HREF come from nomads.ncep.noaa.gov, which sends no
  `Access-Control-Allow-Origin` header (checked with an `Origin` request) and limits how often one
  address may ask, so a shared proxy would be banned. It stays off both allowlists.

**What the browser build does.** `model_browser::regional_unreadable_in_browser` and
`global_unreadable_in_browser` hold the reasons in one place; `unreadable_here` returns them only
on wasm (`the_browser_cannot_read_the_jpeg_2000_and_nomads_models`).
- The model pickers (the Layers panel's model rows, the ribbon's model menu, the model
  timeline's menu) show those models greyed out, the reason on hover: "Desktop and Android only:
  the browser cannot decode its JPEG 2000 files" or "…: its server does not let a browser read
  it".
- Picking one another way (the command palette) is refused with the same reason as a toast.
- Model fields greys out RAP and HREF probabilities in its model menu, and a saved pick of one
  shows the reason instead of a fetch error.
- The point forecast's models use the same reasons (they had their own copy).

**Browser build on WebGPU, one machine**, built from the working tree at `26836bfd` with this
change and another session's uncommitted TGFTP work.

| Capture | What it shows |
| --- | --- |
| `model-panel-rap-browser.jpg` | The Layers panel's model rows: RAP, HiresW ARW and FV3, RRFS, HREF PMM and mean, and GDPS greyed; hovering RAP reads "Desktop and Android only: the browser cannot decode its JPEG 2000 files". Hovering HiresW ARW read "…: its server does not let a browser read it" (seen, not kept). |
| `model-fields-href-prob-browser.jpg` | Model fields' model menu: RAP and HREF probabilities greyed; hovering HREF probabilities reads the NOMADS reason. |

| File | SHA-256 |
| --- | --- |
| model-panel-rap-browser.jpg | `7cfc2e5da5de321acbb605c3a3e3afab2a5e71f6ef4936747d14e9a08dcc379d` |
| model-fields-href-prob-browser.jpg | `86451098e2c951b9b289879c08a44ba8f4f8a7354c2ba9711aa03b6ff095f0b7` |

Not established: a workspace saved on the desktop with one of these as its model and opened in
the browser (the map layer's own fetch would still fail there, with its error), the command
palette refusal in a run, and the desktop and Android builds, where nothing changes.

Open for the maintainer: the other V12 branch, allowing NOMADS behind the proxy, and a JPEG 2000
decoder for the web build, which would bring back the RAP and GDPS there.
