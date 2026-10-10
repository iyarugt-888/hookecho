# Browser walkthrough, 2026-10-10 (1008.md E2, E3, H4)

The browser build of `feat/wsv3-redesign` at `f1ac066` was built with `scripts/web/build.sh` and
served by `hookecho --serve 8080 --web-root web`. It was then driven by hand in the Claude
desktop app's built-in browser (Chromium, 750×922 viewport) on this Windows machine with its
RTX 2060. The app reported the `BrowserWebGpu` backend.

This is one session on one browser and one machine. It is not a cross-browser or device
certification.

## What was checked

| Step | Result | Capture |
| --- | --- | --- |
| Boot | The app came up on KTLX, live, with the welcome dialog. The console showed no panic and no WebGPU error. The only failed request was `/geo.json`, which the local server does not provide. | [boot-ktlx.jpg](boot-ktlx.jpg) |
| Crossfade setting (E2) | Preferences → Data age lists "Crossfade MRMS, model and satellite layers between frames". Turning it on saved `"field_crossfade": true` in local storage. With it on, the MRMS mosaic loaded and the radar loop ran for more than a minute with no panic or GPU error. | [crossfade-setting.jpg](crossfade-setting.jpg) |
| ECMWF runs (E3) | The ECMWF run picker lists 00Z and 12Z runs "to F+360h" and 06Z and 18Z runs "to F+144h". | [ecmwf-runs.jpg](ecmwf-runs.jpg) |
| ECMWF F+360 (E3) | On the 00Z 10 Oct run, dragging the lead to its end gave F+360. The 2 m temperature loaded "valid Oct 24, 7:00 PM CDT", with requested and loaded agreeing. One step back gave F+354. | [ecmwf-f360.jpg](ecmwf-f360.jpg), [ecmwf-f354.png](ecmwf-f354.png) |
| Proxy reach | From the page, `/proxy/data.ecmwf.int/…-360h-oper-fc.index` returned 200 with 184 lines. `/proxy/nomads.ncep.noaa.gov/…` returned 403 "host not proxyable". A direct fetch to NOMADS was blocked by CORS, because NOMADS sends no `Access-Control-Allow-Origin`. | — |

A fade itself was not caught on screen. It lasts 250 ms, and there was little echo near KTLX.
The GPU control in [m5.4](../m5.4/README.md) is the evidence for the blend.

## Findings

1. **HiresW cannot load in the browser build.** HiresW ARW and FV3 are served only from
   `nomads.ncep.noaa.gov`. That host is in neither the proxy allowlist (`serve.rs`
   `ALLOWED_HOSTS`, mirrored in `web/_worker.js/proxy-core.js`) nor `wxdata::net::CORS_OK`. Adding
   it changes the proxy's trust boundary. NOMADS also throttles and blocks busy addresses, and a
   shared proxy would put every visitor's NOMADS requests on one IP. Both are the maintainer's
   decision, so the allowlist is unchanged. Until then, HiresW in the browser fails with the
   proxy's refusal.
2. **The bundle is over its budgets.** Without `wasm-opt` (binaryen is not installed here), the
   wasm is 26,108,976 bytes raw and 6,882,053 gzipped ([build.log](build.log)). The budgets are
   18,000,000 raw and 4,500,000 gzipped. Cloudflare Pages refuses an asset over 26,214,400
   bytes, and this build is 105,424 bytes under that. CI runs `wasm-opt`, which the script says
   trims about 15%, so the deployed size was not measured here. The walkthrough build raised both
   budgets locally (`HOOKECHO_WASM_BUDGET`, `HOOKECHO_WASM_RAW_BUDGET`) and did not build the
   lite page, because `jq` is missing.

## Hashes

| File | SHA-256 |
| --- | --- |
| boot-ktlx.jpg | `b7364b211b6d6ad524d5a598187b47b4dbafd6abb95dfdf212993e2456c9dbc6` |
| crossfade-setting.jpg | `f8df3aaa091619e583bee9a5c11b376a13f897c62d0655f943bada3d7e4a1de2` |
| ecmwf-runs.jpg | `8fe528f385976d0d185ed255d8f6c43284fe85dc4c7d02d0ed42c4af3192b93f` |
| ecmwf-f360.jpg | `63ccf0c3df6e08564c14040384ed69969dfe629c152118ad2da2f09f0967a34d` |
| ecmwf-f354.png | `f74bf303cf11b3f63279765c549162aacf58bcbc194f678d87b0705228a073ad` |
| build.log | `025ee3d9ee13a1e22a2ab0778f5d8c705f6aa234d3487c482711ea19b13a6a58` |
