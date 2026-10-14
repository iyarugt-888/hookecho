# Certification evidence (ROADMAP_PARITY M7.3, 1008.md H4)

Current priorities and continuous validation tracks: [roadmaps](../../roadmaps/README.md).

Increment evidence outside the historical card ledgers: [completed fallback](live-fallback/README.md)
and [accepted-revision timing](live-trace/README.md), plus the
[Codex/Claude handoff verification](handoff-2026-10-14/README.md) and
[progressive source-clock freshness](live-source-clock/README.md) and
[displayed-subscription feedback](display-feedback/README.md). Their local manifests include dirty-tree
checks; the historical manifest below describes committed evidence only.

**Not a release certification.** This index gathers the evidence reviewed so far. Release
readiness is **not claimed**, for these reasons:

- No Android device evidence for any card (M7.2 open).
- Presentation (scan-out) latency is not observed anywhere (M1.2).
- Renderer-only soaks do not certify full application and Android lifecycle profiles (M7.1/M7.2).
- Operational independent-provider failover remains unverified; loopback continuation is not certification (M1.3).
- Several workflows are shown by unit tests and offscreen captures only, not interactive walkthroughs.

[manifest.json](manifest.json) (schema 1) lists every tracked evidence file under
this folder with its SHA-256 and size. It was generated at 2026-10-14T05:12:53+00:00 from commit
`4bf574993215` on `feat/wsv3-redesign`. Measurement host, unless a file says
otherwise: Windows 11 Home 10.0.22631, NVIDIA GeForce RTX 2060 (Vulkan). Exact bytes as stored in the recorded Git commit, including each artifact's stored line endings; checkout conversion can change working-tree bytes.

Regenerate it after adding evidence:

```bash
python scripts/certification/manifest.py
python scripts/certification/readme.py
```

## Cards

The ledger count (dated entries, counted by their headings) and latest date come from each card's evidence ledger in `ROADMAP_PARITY.md`,
which holds the commands, results and open items. Every card's Android gate is open unless its
ledger records a physical-device run.

| Card | Title | 1008.md | Ledger entries | Latest | Evidence |
| --- | --- | --- | --- | --- | --- |
| M0.1 | Reconcile and maintain the evidence baseline | — | 0 | — | — |
| M0.2 | Make domain ownership testable | — | 1 | 2026-10-01 | — |
| M0.3 | Pin the scientific and visual corpus | — | 5 | 2026-10-02 | [m0.3/](m0.3/) |
| M1.1 | Carry temporal coverage into every radar representation | A5 | 15 | 2026-10-11 | [m1.1/](m1.1/) |
| M1.2 | Trace receipt through completed rendering | A3 | 4 | 2026-10-10 | [m1.2/](m1.2/) |
| M1.3 | Prove source failover under severe-weather load | A1, A2 | 7 | 2026-10-11 | [m1.3/](m1.3/) |
| M1.4 | Complete retained provenance and scientific lineage | A1, A4 | 6 | 2026-10-09 | [m1.4/](m1.4/) |
| M2.1 | Introduce persistent storm identity and history | B1 | 4 | 2026-10-06 | — |
| M2.2 | Unify storm selection, trends, and tool entry | B1 | 7 | 2026-10-10 | [m2.2/](m2.2/) |
| M2.3 | Extend impacts to geolocated communities and assets | B2, H3 | 6 | 2026-10-12 | [m2.3/](m2.3/) |
| M2.4 | Certify editing and preserve historical manual work | B3 | 3 | 2026-10-10 | [m2.4/](m2.4/) |
| M3.1 | Expose authoritative gate metadata | C3 | 6 | 2026-10-12 | [m3.1/](m3.1/) |
| M3.2 | Complete portable, validated product definitions | C4 | 4 | 2026-10-12 | [m3.2/](m3.2/) |
| M3.3 | Render and export column-based products | C4 | 4 | 2026-10-06 | [m3.3/](m3.3/) |
| M3.4 | Preserve physical extrema while aging trails | C1 | 6 | 2026-10-09 | [m3.4/](m3.4/) |
| M3.5 | Add translucent volume rendering and richer surfaces | C2 | 4 | 2026-10-09 | [m3.5/](m3.5/) |
| M3.6 | Link storm-centered 3D, slices, and measurement | C5 | 12 | 2026-10-11 | [m3.6/](m3.6/) |
| M4.1 | Introduce independent GIS layers and groups | D3 | 1 | 2026-10-06 | — |
| M4.2 | Complete native imports with shapefile bundles | D1 | 6 | 2026-10-09 | [m4.2/](m4.2/) |
| M4.3 | Finish styling, feature inspection, and filtering | D2 | 8 | 2026-10-09 | [m4.3/](m4.3/) |
| M4.4 | Restore and exchange GIS scenes reliably | D3 | 9 | 2026-10-09 | [m4.4/](m4.4/) |
| M5.1 | Support independent link groups and source contexts | E1 | 9 | 2026-10-12 | [m5.1/](m5.1/) |
| M5.2 | Add satellite-native one-minute playback | E2 | 7 | 2026-10-12 | [m5.2/](m5.2/) |
| M5.3 | Broaden field inspection within supported models | E3 | 19 | 2026-10-12 | [m5.3/](m5.3/) |
| M5.4 | Unify rendering quality, labels, and transitions | E2, E4 | 10 | 2026-10-12 | [m5.4/](m5.4/) |
| M6.1 | Separate preview from program output | — | 2 | 2026-10-06 | — |
| M6.2 | Persist complete, validated scenes | F1 | 5 | 2026-10-10 | [m6.2/](m6.2/) |
| M6.3 | Harden deterministic capture and archive output | F2 | 6 | 2026-10-12 | [m6.3/](m6.3/) |
| M6.4 | Deliver usable desktop and Android presentation | F3 | 4 | 2026-10-12 | [m6.4/](m6.4/) |
| M7.1 | Extend soaks to the full application and resources | H2 | 4 | 2026-10-11 | [m7.1/](m7.1/) |
| M7.2 | Certify Android lifecycle, input, and memory | H1 | 2 | 2026-10-10 | — |
| M7.3 | Publish certification and operational documentation | H4 | 1 | 2026-10-09 | — |

## Operator guides

| Guide | Covers |
| --- | --- |
| [GUIDE.md](../GUIDE.md) | Using the app |
| [TROUBLESHOOTING.md](../TROUBLESHOOTING.md) | Recovering failed workflows and stale data |
| [DATA.md](../DATA.md) | Data sources |
| [time-alignment.md](../time-alignment.md) | How layers are matched in time |
| [obs-guide.md](../obs-guide.md) | The output window with OBS |
| [spotter-network.md](../spotter-network.md) | Why reports are not submitted to Spotter Network |

These guides have not been reviewed against every workflow added in this round. That review is
part of the open M7.3 work.
