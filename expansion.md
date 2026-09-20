# Expansion — desktop and Android, off the web build's budget

**Status:** plan, 2026-09-20. Written against `feat/wsv3-redesign` at `3e19b46`, version
`0.12.0-beta.2`.

## Why this document exists

The browser build is the constrained one. Everything a first-time visitor to
`app.hookecho.io` downloads before there is radar on screen is paid for on the wire, every
time, and `scripts/web/build.sh` enforces that with a gzip budget. That budget has been raised
twice today alone (`c718732` to 4.3 MB, `ff1d217` to 4.5 MB), and the comment in `build.sh`
says the slack is "roughly one more release at that observed rate."

The desktop and Android builds have no such constraint and no gate at all. Their real ceilings
are between **15x and 19x** the size they ship at today. The web build is not the product's
ceiling — it is one deployment target that happens to be the smallest, and the roadmap has been
paying its tax on every platform.

This plan does three things:

1. Establishes what the actual maximum is on Android and Windows, measured and sourced.
2. Specifies the gates that keep native growth structurally out of the web bundle, so
   `app.hookecho.io` keeps deploying no matter what lands natively.
3. Lists what to build on native, grouped by the browser limitation that makes it native-only,
   with every item checked against the no-third-party-approval constraint.

It does not restate [ROADMAP_NEW.md](ROADMAP_NEW.md). Where an item here already has a roadmap
ID, the ID is cited and the roadmap remains the source of truth for scope.

---

## 1. Where things stand, measured

Release asset sizes, read from the `latest` release on 2026-09-20:

| Artifact | Bytes | MiB |
|---|---:|---:|
| `HookEcho-arm64-v8a.apk` | 113,840,632 | 108.6 |
| `HookEcho-x86_64.msi` | 132,603,103 | 126.5 |
| `HookEcho-setup-x86_64.exe` | 89,342,501 | 85.2 |
| `hookecho-windows-x86_64.zip` | 94,594,654 | 90.2 |
| `HookEcho-x86_64.AppImage` | 100,231,672 | 95.6 |
| `HookEcho-amd64.deb` | 89,901,860 | 85.7 |

Web, gzipped, as recorded in `build.sh` from a real container build at `ff1d217`:

Web, measured at HEAD (`3e19b46`) with the pinned Binaryen 130, reproducing CI:

| Bundle | Raw | Gzipped | Gzip budget | Used |
|---|---:|---:|---:|---:|
| `hookecho_bg.wasm` | 11,513,495 | 4,271,208 | 4,500,000 | 94.9% |
| `lite_bg.wasm` | — | 73,128 (local) | 80,000 | 91.4% |

Both web bundles are inside 10% of their gates. The native artifacts are nowhere near anything.

**The dependency graphs are already well separated.** `cargo tree --edges normal --no-dedupe`,
deduplicated by crate:

| Target | Crates in the graph |
|---|---:|
| `wasm32-unknown-unknown` | 253 |
| `aarch64-linux-android` | 365 |
| `x86_64-pc-windows-msvc` | 380 |

About 110 crates are already excluded from the web build by the per-target dependency tables in
`crates/hookecho/Cargo.toml` and the 262 `cfg` fences in `crates/hookecho/src`. The mechanism
this plan depends on is not new — it is the mechanism the repo already runs on. What is missing
is anything that *measures* it.

---

## 2. The actual ceilings

### 2.1 Web — the wall is not the one being gated

The gzip budget is a policy number. The hard wall is Cloudflare Pages, which `demo.yml`
deploys to via `wrangler pages deploy`:

> The maximum file size for a single Cloudflare Pages site asset is 25 MiB.

That is **26,214,400 bytes, uncompressed**, on `hookecho_bg.wasm`. A build that exceeds it does
not get a slow first paint — the deploy fails and `app.hookecho.io` stops updating.

Nothing in the repo measured the raw size — `build.sh` printed it and gated only the gzip
number. **Measured at HEAD (`3e19b46`) with the pinned Binaryen 130: 11,513,495 raw, 43.9% of
the wall, 14.7 MB of headroom.** The raw gate in section 4.1 has since been added and is set at
18,000,000, well below the wall, so the warning arrives while there is still room to think.

Two things that measurement settled, both worth recording because both were guessed wrong first:

- The gzip gate is **not** failing. The same build measures 4,271,208 gz against the 4,500,000
  budget — 94.9%, 228,792 bytes of slack, and within 711 bytes of the number `build.sh` records
  for the container build at `ff1d217`. That number is trustworthy.
- **A local build without Binaryen now overstates the bundle by ~750 KB gzipped**, and
  `build.sh`'s comment said the opposite. At 18.6 MB raw, wasm-opt wins on both axes
  (18,621,027 raw / 5,019,747 gz without it, against 11,513,495 / 4,271,208 with it — 38% and
  15%). The old note was measured at ~12 MB raw, where the entropy cost of optimization still
  dominated the dead-code saving; it has since inverted. A developer trusting it would see a
  local build 750 KB "over budget" and go cut features that were never the problem. Corrected
  in `build.sh` in the same pass as this document.

Cloudflare Pages also caps a site at 20,000 files on the free plan. `web/dist` holds a dozen.
Not a concern.

### 2.2 Android

There are four distribution paths, with four different ceilings:

| Path | Ceiling | Needs approval? |
|---|---|---|
| Sideload from GitHub Releases (current) | **2 GiB** per release asset | No |
| F-Droid | No documented size limit; **2-hour build timeout** binds first | Yes (fdroiddata MR review) |
| Google Play, as an App Bundle | **500 MB** base module, compressed download | Yes |
| Google Play, as a legacy APK | **100 MB** — already exceeded | Yes |

Detail on the Play numbers, from the Play Console size-limits page: base module 500 MB, each
feature module 500 MB, each asset pack 1.5 GB, all modules plus install-time asset packs 4 GB
combined, on-demand and fast-follow packs 30 GB, 34 GB overall. All measured as *compressed
download size as Play Console computes it*. Apps over 1 GB must target API 21+, which minSdk 29
already satisfies. Apps over **200 MB** show users on mobile data a non-blocking
large-download dialog.

**The operative number today is 2 GiB**, because HookEcho ships a sideloaded APK and does not
need Play. At 113.8 MB that is **18.9x headroom**. The 108.6 MiB APK already exceeds Play's
legacy-APK limit, so if Play is ever wanted it is an App Bundle conversion, not a size cut —
and that is a third-party approval, which this plan does not assume.

For F-Droid the size limit is irrelevant and the **build time** is the real budget. F-Droid's
buildserver terminates a VM at `build_timeout`, default 7200 seconds. HookEcho's aarch64
release build runs `lto = "fat"` with `codegen-units = 1` on a workspace of 365 crates plus a
vendored GRIB decoder; `Dockerfile.coolify` already warns "budget 20–40 minutes for a cold
build" for the native binary plus two wasm builds on a normal host. F-Droid's VM is slower.
Every native-only crate added to the Android graph spends that budget.

### 2.3 Windows

| Constraint | Ceiling |
|---|---|
| GitHub release asset | **2 GiB** |
| Inno Setup single-file installer | **~2,100,000,000 bytes** without disk spanning |
| MSI | **2 GB** per CAB (a format limit, not a WiX one) |
| EXE with embedded resources | 4 GB (Windows) |

The binding number is effectively **2 GB across all three**. Against the 85.2 MiB setup EXE
that is **~23x headroom**; against the 126.5 MiB MSI, about 15x.

Note the MSI is 43 MB larger than the Inno EXE for the same payload — MSI/CAB compression is
worse. If native growth ever makes size matter on Windows, switching the MSI to an
uncompressed-media layout, or dropping it in favour of the EXE plus winget, is the lever, and
it is worth 40 MB today.

### 2.4 Runtime ceilings, which bind long before file size

File size is not the interesting constraint on either native platform. These are:

- **Android native memory.** Rust allocations do not come from the ART heap and are not bound
  by `getMemoryClass()`. They are bound by physical RAM and the low-memory killer, and Android
  has no swap. There is no per-app number to design against — the app is killed when the device
  is under pressure. `crates/hookecho/src/platform.rs` has no `onTrimMemory` path today;
  nothing drops caches when the OS asks. ROADMAP_NEW O1 lists "memory budget by device class"
  for Android as open, and it is the single most important budget to establish before shipping
  memory-hungry native features to phones.
- **Android foreground-service time.** See section 6.2 — this is a live bug, not a budget.
- **Build time.** F-Droid 2 h; GitHub Actions 6 h per job; Cloudflare Pages' own 20-minute
  build timeout does not apply, because `demo.yml` builds in Actions and only uploads.
- **Desktop GPU and thread budget.** ROADMAP_NEW O1's desktop targets (60 fps pan/zoom, no
  >16 ms UI-thread decode stalls) are the real gate on everything in section 5.

---

## 3. Proposed budgets

Hard ceilings this far away are not useful as gates — a gate at 2 GB never fires. These are the
numbers to actually adopt, chosen so that a careless regression fails and a release's worth of
deliberate feature work does not.

| Target | Soft gate (CI warns) | Hard gate (CI fails) | Hard ceiling | Rationale |
|---|---:|---:|---:|---|
| Web `hookecho_bg.wasm`, gzip | 4,500,000 | — | — | keep the existing number and `build.sh`'s existing philosophy |
| Web `hookecho_bg.wasm`, **raw** | — | **18,000,000** *(shipped)* | 26,214,400 | the Cloudflare wall, gated far below it so the warning arrives with room to think; measured 11,513,495 |
| Web `lite_bg.wasm`, gzip | — | 80,000 | — | unchanged; it is at 91% and that is the point of the page |
| Android APK | 175,000,000 | **200,000,000** | 2 GiB | the Play mobile-data warning line is the natural stop, keeps the AAB door open, and is 1.75x today |
| Android `libhookecho.so` build, wall clock | 45 min | **90 min** | 7200 s (F-Droid) | measured in the `android-check` CI job |
| Windows setup EXE | 200,000,000 | **500,000,000** | ~2.1 GB | download time and AV scanning bind long before Inno does |
| Android peak RSS, 6 GB-class device | — | *to be set* | physical RAM | **measure first.** ROADMAP_NEW O1 wants this; do not invent a number here |

Two of these matter more than the rest: the **raw** web gate, because it guards an actual
deploy failure that nothing currently watches, and the **Android build-time** gate, because it
is the budget native expansion will spend fastest and the only one whose overrun is invisible
until an F-Droid build is killed at two hours.

---

## 4. Keeping it separate

The requirement is that native expansion cannot make the web deploy fail. Three gates, in
ascending cost.

### 4.1 Gate the raw wasm size — `scripts/web/build.sh` — **done**

Shipped as `HOOKECHO_WASM_RAW_BUDGET`, default 18,000,000, in the block after the gzip gate.
The gzip budget is a policy about the visitor's wire cost and can be raised by whoever is
willing to pay it; this one is not a policy, because past 26,214,400 the deploy simply does not
happen. Same pass corrected the stale Binaryen note above it (see section 2.1).

**Cost:** ten minutes. **Bought:** the failure mode nobody was watching, and a local-build
caveat that had inverted without anyone noticing.

### 4.2 Gate the web dependency graph — new step in `ci.yml`

`cargo check --target wasm32-unknown-unknown` catches a native-only module that does not
compile for wasm. It does not catch the failure `build.sh`'s own comment names as the reason
the gate exists: "a careless dependency (hundreds of KB, minimum)" that compiles everywhere and
lands in the browser bundle by accident. The size gate catches that only after the fact, as a
number nobody can attribute without running `scripts/web/bloat.sh`.

Commit the web dependency graph and diff it, in exactly the idiom `demo.yml` already uses for
the proxy allowlist:

```sh
# web/deps.lock, regenerated by scripts/web/deps.sh
cargo tree --target wasm32-unknown-unknown -p hookecho --edges normal --prefix none --no-dedupe \
  | sed 's/ (\*)$//' | sort -u > web/deps.lock
```

253 lines today. CI diffs the generated list against the committed one and fails on drift, with
a message saying that adding a crate to the browser bundle is a deliberate act and this file is
where you record it. A native-only dependency does not appear in this list at all, so native
expansion never touches it.

**Cost:** an afternoon. **Buys:** attribution at the moment of the change rather than
archaeology at release time, and a review surface — the diff shows up in the PR.

### 4.3 Make native-only the default for new code — `crates/hookecho/src/native/`

262 `cfg(target_arch = "wasm32")` fences means the default for a new module is *ships on web*,
and staying off the web bundle is an opt-out someone has to remember. For an expansion whose
whole premise is native-only work, that default is backwards.

Declare one subtree, fenced once in `lib.rs`:

```rust
/// Everything that only exists on desktop and Android. One fence, not one per module: new work
/// in the expansion tracks lands here by default and cannot reach the browser bundle.
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
```

New expansion modules go under `native/`. Existing modules are not moved — ARCHITECTURE.md
already warns that `app.rs` is a god file and that splitting it is welcome but state placement
matters, and a 262-site refactor to prove a point is the wrong trade. This is a convention for
new code with one enforcement point.

**Cost:** small, and mostly discipline. **Buys:** the separation stops depending on every
future author remembering the fence.

### 4.4 What already works and should not be touched

- Per-target dependency tables in `crates/hookecho/Cargo.toml`. The comments there
  (`accesskit`, `default_fonts`, `rodio`, `image`'s `ico`/`gif`) are a record of exactly this
  discipline being applied correctly.
- The single-number gate in `build.sh` and the deleted `Dockerfile.coolify` override. The
  commit that removed the second copy (`c718732`) got the principle right; do not reintroduce a
  per-deployment budget.
- `web/lite` as the floor. It is a real answer for machines that cannot run the app, and its
  80 KB budget is doing its job at 91% occupancy.

---

## 5. What to build on native

Grouped by the browser limitation that makes each one native-only, so the separation is a
property of the feature rather than a policy applied to it. Roadmap IDs cite
[ROADMAP_NEW.md](ROADMAP_NEW.md).

### 5.1 No filesystem — storage-unbounded work

The web build caps its IndexedDB store at 250 MB (`webcache.rs` `BYTE_CAP`) with a further
150 MB for the automatic archive cache. Desktop has a disk; Android has an app-private dir.

- **Local archive library.** Level II back to June 1991 is on `noaa-nexrad-level2` with no key.
  A desktop case library — fetched once, kept, indexed, searchable by date, site and event — is
  gigabytes, and is the single largest thing the browser structurally cannot do. Feeds K3
  (case-study package) and K4 (analyst notebook/export).
- **Offline chase pack v2** (L5), and satellite chase packs (E7). On Android this is the
  strongest feature available: the pack is what the app has when the signal does not. The web
  build already has a cut-down version; the native one has no cap.
- **Decode and regrid caching** (O3) at all four levels — compressed bytes, decoded native
  grid, regridded display form, GPU texture. Only the first tier is affordable in a browser.

### 5.2 No process spawning — the extension path

- **Stable plugin manifest** (P1) and the safe product plugin path (P2). The runner exists
  (`plugins.rs`); documenting the manifest turns it into the analyst extension story. Language
  independent, OS-isolated, no new dependencies, and — importantly here — no vendor.
- **Python interoperability** (P3). The approval-free answer to every "can I script this"
  request, and it costs the web bundle exactly nothing.
- **Broadcast output workspace** (M1) and deterministic capture (M2) — but see section 7 on
  NDI.

### 5.3 No sockets — the local-network product

- **Local API** (M4). `--serve` exists and `custom_components/` already ships a Home Assistant
  integration. Documenting a local HTTP/WS API makes HookEcho a source other software on the
  user's own network can consume, with no account and no server of the project's.
- **Self-hosted relay** — `radar-ingest`, `relay_provider.rs`, `provider_health.rs`,
  `radar_provider_manager.rs` (B6.11). Already native-only by construction; the comments in
  `lib.rs` say so. Expand here freely.
- **MQTT** (`mqtt.rs`) and UDP position sharing. Both already desktop-only.

### 5.4 No threads — CPU-heavy analysis

`rt.rs` notes that wasm `spawn_blocking` is `spawn_local` on the main thread, and that the
ceiling is jank on big CPU work. Everything below is native by arithmetic, not by policy.

- **Model workstation breadth** (F2 Tier 1). GFS beyond the current comparison fields, RRFS
  deterministic, REFS/RRFS ensemble, NBM on a field-based surface. All on NODD/NOMADS, no key,
  no approval. GRIB decode weight is exactly the kind of dependency the web budget cannot
  absorb — `gribberish` is vendored and `wxdata` is pinned to `opt-level = 3` in the web profile
  precisely because decode cost is already the browser's problem.
  *RRFS/REFS v1 operational implementation is listed for 2026-10-06; build against the parallel
  feed and do not assume final naming until the provider contract tests pass.*
- **Multi-radar storm volume fusion** (R1) and **dual-Doppler wind synthesis** (R2).
- **Object-based storm history** (R3) and **feature tracking** (R4).
- **3D workstation** (H1–H7). `registry.rs` already hides `W::Volume3d` on wasm, and
  `volume3d_window.rs` already drops Android to 96 raymarch steps against desktop's 256. The
  gating is in place; the features are not built.
- **Ensemble workstation** (F7), **point sounding overhaul** (F8), **RGB recipe engine** (E4).

### 5.5 Android-specific

- **Tablet and foldable layout** (Q1). The mobile chrome in `app/mobile/` assumes a phone.
- **Widget and quick-tile expansion.** `AlertWidget`, `RadarWidget` and `RadarTile` exist and
  are thin. More surfaces, no new permissions.
- **Radar-derived alerting.** Today `AlertService.kt` polls `api.weather.gov` for the saved
  markers. Rotation and hail proximity from MRMS is a genuinely different product and needs
  decode in the background — which means the Rust side, not more Kotlin. Watch the field-name
  contract (`kotlin_alert_service_field_names_survive` is the only compiler that sees both sides
  of `settings.json`), and resolve section 6.2 before adding any background work at all.

### 5.6 Desktop-specific

- **Analyst density** (Q2), **keyboard-first workflows** (J6), **analyst presets** (J5).
- **Algorithm laboratory** (C5) and **user-defined product engine** (C1) depth.

---

## 6. Fix before expanding Android

Two of these are correctness problems on 2026 devices. Expanding Android on top of them would
be building on a floor that is already cracked.

### 6.1 16 KB page alignment — NDK r26d is too old

Android 15+ devices can run with 16 KB memory pages. Native libraries whose ELF LOAD segments
are 4 KB-aligned fall into **16 KB backcompat mode**: the app shows a warning on first launch
and runs with, in Google's words, reduced reliability and stability.

- **NDK r28 and higher align at 16 KB by default.** `android/build.sh` and the `android-check`
  CI job both pin **r26d**, and the F-Droid recipe in `android/README.md` names `ndk: r26d`.
- AGP 8.5.2 (`android/build.gradle.kts`) is above the 8.5.1 floor, so the *packaging* half —
  uncompressed, 16 KB zip-aligned `.so` — is already right. Only the link is wrong.
- For r27 and below the fix is linker flags:
  `-Wl,-z,max-page-size=16384 -Wl,-z,common-page-size=16384`.

**Do:** either move to NDK r28+ in `build.sh`, in `ci.yml`'s `android-check`, and in the
F-Droid recipe, or add the two linker flags via `RUSTFLAGS` on the `cargo-ndk` invocation. Then
verify with `zipalign -c -P 16 -v 4 app-release.apk` and add that check to the release workflow
beside the existing dangling-C++-symbol check, which is the same kind of guard.

This is a *device* problem, not only a Play problem. Google's requirement (all Play submissions
targeting API 35+ since 2025-11-01) does not apply to sideloaded APKs — but backcompat mode and
its warning dialog apply to any app on a 16 KB device, however it was installed.

### 6.2 Android 15 foreground-service timeout — a live crash

`AndroidManifest.xml` declares `android:foregroundServiceType="dataSync"` and `targetSdk` is
35. On Android 15 that means **6 hours of cumulative `dataSync` foreground service per 24-hour
window**. At the limit the system calls `Service.onTimeout()` and gives the service seconds to
call `stopSelf()`. If it does not, the system throws
`android.app.RemoteServiceException: "A foreground service of type dataSync did not stop within
its timeout"` — a fatal exception.

`AlertService.kt` has no `onTimeout` override. A user who leaves background alerting on through
a long severe-weather day reaches six cumulative hours and the app crashes.

Mitigating factors: `AlertAlarm` and the 15-minute `AlertWorker` are independent legs, so
alerting degrades rather than stopping; and bringing the app to the foreground resets the
window. Neither prevents the crash.

**Do:**

1. Override `onTimeout(startId, fgsType)` in `AlertService`, stop cleanly, and record the
   reason in the delivery-health line `platform::alert_health` already parses, so the user is
   told why the persistent notification went away.
2. Consider whether `dataSync` is even the right type. The service polls for and delivers
   weather warnings; `specialUse` with a declared justification is a closer description and is
   not subject to the 6-hour cap. It is Play-policy-sensitive, which for a sideload/F-Droid app
   is a documentation question rather than a review one — but write the justification down
   either way.
3. Test with `adb shell am compat enable FGS_INTRODUCE_TIME_LIMITS io.hookecho.HookEcho`, which
   forces the limit without waiting six hours.

### 6.3 Memory pressure — nothing listens

`platform.rs` gates background work on foreground state (eframe stops calling `update()` when
Android tears down the surface), which is the right instinct applied to CPU. Nothing does the
equivalent for memory: no `onTrimMemory`/`onLowMemory` path, so no cache is dropped when the OS
asks and the process is simply killed.

**Do:** before any of section 5.1's storage work reaches Android, bridge `onTrimMemory` through
JNI into a cache-eviction hook, and establish the device-class memory budget ROADMAP_NEW O1
asks for by measuring, not by guessing.

### 6.4 The APK is arm64-v8a only

Deliberate ("every phone that can run this ships it") and correct for phones. Worth noting only
because it closes two doors: x86_64 Android emulators, and Chromebooks running Android apps.
Neither is worth an ABI split today; revisit if the build-time budget in section 3 turns out to
have room.

---

## 7. The no-third-party-approval constraint

Everything in section 5 is reachable without anyone's permission. This section records what
that rules out and what replaces it, so the line does not have to be re-litigated per feature.

### 7.1 Data sources

ARCHITECTURE.md already states the rule: "No telemetry, no accounts, no server of ours. A
change that adds a hosted dependency is a change to the product, not just the code."
[docs/DATA.md](docs/DATA.md) shows it is being kept — every feed is public, and the three that
need a key (mPING, AirNow, and the user's own PWS) are free and the user's own.

| Tempting vendor capability | Requires | Approval-free substitute, already wired |
|---|---|---|
| NLDN / Vaisala cloud-to-ground lightning (AllisonHouse, Baron, Earth Networks) | paid contract | **GOES GLM** on AWS (`glm.rs`, 20 s cadence, ~40 s latency) plus MRMS lightning products |
| Vendor-processed or dealiased Level II | paid contract | Unidata chunk stream, the project's own `radar-ingest` relay, and `dealias.rs` |
| Vendor placefile subscriptions | paid contract | GRLevelX placefile format rendered natively (`placefile.rs`); Spotter Network placefile |
| Vendor model grids and "exclusive" guidance | paid contract | NODD / NOMADS — HRRR, RAP, NAM, GFS, GEFS, NBM, RRFS/REFS, all free, no key |
| Commercial road and traffic data | paid contract | DOT camera feeds (`dotcams.rs`), state 511 feeds where public |

**The rule to apply to any new source:** if reaching it requires a credential that is not the
user's own and not free-on-signup, it is a change to the product. The roadmap's own Tier 2 note
on ECMWF says the same thing ("only where openly and legally retrievable"), and F2's warning —
"do not depend on fragile scraped images when machine-readable grids exist" — is the other half
of it.

One caution worth writing down: **NWWS-OI** and NODD's **SNS/SQS** push notifications are both
free and both look approval-free from a distance. NWWS-OI requires registration with the NWS;
SQS requires an AWS account. Neither is a vendor contract, but neither is "the app talks to
NOAA directly" either. Polling the same data from the open buckets stays inside the product's
stated model.

### 7.2 Distribution channels

| Channel | Approval | Status |
|---|---|---|
| GitHub Releases (Windows EXE/MSI/zip, APK, AppImage, deb) | **none** | **the primary path; all of section 5 ships here** |
| Cloudflare Pages (`app.hookecho.io`, `hookecho.io`) | none — own account | current |
| F-Droid | fdroiddata MR review | recipe drafted in `android/README.md`, not submitted |
| IzzyOnDroid | lighter third-party review | not pursued |
| winget / Homebrew / AUR / Flatpak | per-repo PR review | manifests stamped by `stamp-manifests.sh` |
| Google Play | review plus policy | **not required, not assumed** |
| Microsoft Store | review | not pursued |

The plan assumes only the first two. Everything else is optional and none of it gates section 5.

Two related items that are not "approval" but are third-party dependencies with a cost, worth
naming so they are not discovered late:

- **Windows code signing.** README already tells users to click through "More info → Run
  anyway." A certificate removes that, and every route to one (a CA, or Azure Trusted Signing)
  involves identity validation by a third party. Out of scope here; keep the README note.
- **NDI, for M1's broadcast output.** The NDI SDK ships under a license agreement. If M1 wants
  approval-free output, the shape is a borderless output window plus frame-file output plus the
  existing `--serve` snapshot endpoint — all of which the codebase can already nearly do.

---

## 8. Sequencing

**Track 0 — gates (do first, days).** Everything else is safer behind these.

1. Section 4.1 raw wasm gate in `build.sh`.
2. Section 4.2 `web/deps.lock` drift check in `ci.yml`.
3. APK size gate and build-time report in the release and `android-check` workflows, at the
   section 3 numbers.
4. Section 4.3 `src/native/` declared and documented in ARCHITECTURE.md's extension-points
   list.

**Track 1 — Android floor (do before Android features, ~1–2 weeks).**

5. Section 6.1 16 KB alignment: NDK bump or linker flags, plus a `zipalign -c -P 16` check in
   CI.
6. Section 6.2 `onTimeout` in `AlertService`, and the FGS-type decision written down.
7. Section 6.3 `onTrimMemory` bridge, then measure and set the Android memory budget (O1).

**Track 2 — native capability, in the order that compounds.**

8. Section 5.1 local archive library and decode/regrid cache tiers (O3) — every later analysis
   feature reads from these.
9. Section 5.4 model breadth (F2 Tier 1), the largest user-visible gap and the clearest case of
   the web budget having suppressed native work.
10. Section 5.3 local API (M4) and relay depth (B6.11) — these make the desktop build a source
    rather than only a viewer.
11. Section 5.4 fusion and tracking (R1–R4), section 5.2 plugin manifest (P1/P3).

**Track 3 — platform surfaces.** Section 5.5 Android tablet layout and widgets, section 5.6
desktop density, in parallel with Track 2 as capacity allows.

**Web, throughout:** feature-frozen. Not deprecated, not neglected — the lite viewer and the
main bundle both stay working and both stay gated. But the browser stops being the reason a
native feature does not get built, which is the entire point of this document.

---

## 9. How to tell whether this worked

- `app.hookecho.io` deploys on every push to main for the whole period, and the raw-size gate
  from section 4.1 never fires. If it fires, the separation leaked and section 4.2's diff says
  where.
- `web/deps.lock` changes only in commits whose message says the browser bundle is meant to
  change.
- `HOOKECHO_WASM_BUDGET` is not raised again during Track 2. The budget has moved twice in one
  day; the measure of success is that it stops moving, because native work stopped pushing on
  it.
- The APK and Windows gates in section 3 are boring — set, reported in CI, never argued about.
- The Android build-time number is *reported on every CI run*, so the F-Droid conversation can
  start from a measurement rather than from a failed two-hour build.

---

## Sources

Ceilings in section 2 are quoted from:

- [Cloudflare Pages limits](https://developers.cloudflare.com/pages/platform/limits/) — 25 MiB
  per asset, 20,000 files on the free plan.
- [Google Play app size limits](https://support.google.com/googleplay/android-developer/answer/9859372?hl=en)
  — 500 MB base module, 1.5 GB asset pack, 4 GB combined, 100 MB legacy APK, 200 MB mobile-data
  warning.
- [About GitHub releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
  — 2 GiB per asset, 1000 assets per release.
- [F-Droid Build Server Setup](https://f-droid.org/docs/Build_Server_Setup/) — `build_timeout`,
  default 7200 s.
- [Support 16 KB page sizes](https://developer.android.com/guide/practices/page-sizes) — NDK
  r28 default alignment, AGP 8.5.1, backcompat mode, `zipalign -P 16`.
- [Foreground service timeouts](https://developer.android.com/develop/background-work/services/fgs/timeout)
  and [Behavior changes: Android 15](https://developer.android.com/about/versions/15/behavior-changes-15)
  — 6-hour `dataSync` limit, `onTimeout`, `RemoteServiceException`.
- [Memory allocation among processes](https://developer.android.com/topic/performance/memory-management)
  — native heap bound by physical memory, no swap.
- [MSI size limits](https://www.advancedinstaller.com/user-guide/qa-installer-large-resources.html)
  — 2 GB CAB, 4 GB EXE-with-resources.
- Inno Setup's own limit (~2,100,000,000 bytes without disk spanning) is reported by its
  compiler error; see the [Inno Setup group thread](https://groups.google.com/g/innosetup/c/8BLzz8LXRhQ).

Measurements in section 1 are from the `latest` GitHub release read on 2026-09-20, the numbers
recorded in `scripts/web/build.sh` at `ff1d217`, and `cargo tree` run against this checkout.
