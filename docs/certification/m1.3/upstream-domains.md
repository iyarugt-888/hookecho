# Declared radar acquisition dependencies

This M1.3 increment adds dependency disclosure to comparative radar health and the existing
diagnostics export. Direct Unidata acquisition declares `unidata-level2-aws`; completed NOAA
fallback declares `noaa-level2-tgftp`. These identify adapter service families, not audited
infrastructure boundaries. A relay's display name, block `source_id`, URL and capabilities do
not establish its acquisition dependency.

Sources now reports the primary and relay declarations, their relationship, and the metadata
check time. Shared identifiers disclose a common declared upstream. Different identifiers
remain explicitly qualified: independent redundancy is not established. Missing declarations,
empty domain lists and failed metadata requests remain unknown. A failed reconnect refresh
replaces the current declaration with unknown; at most one previous declaration remains with
its original timestamp and an explicit historical label. Metadata never updates data receipts,
source-clock freshness, recovery observations or transport success/failure counts.

## Deployment and protocol

`radar-ingest` adds `GET /provider`, independent of `/health`, `/ready`, `/metrics` and the
existing radial WebSocket/HTTP protocol. The optional environment setting
`RADAR_INGEST_UPSTREAM_DOMAINS` supplies comma-separated, non-secret dependency IDs. Whitespace
around IDs is trimmed; at most eight IDs are accepted, each 1–96 bytes, containing lowercase
ASCII letters, digits, `-`, `_` or `.`. Duplicate IDs are removed and the result sorted.
Invalid configuration fails startup without echoing the supplied value. Operators must keep
these identifiers non-secret; syntax validation cannot recognize arbitrary secret text.

For a replay deployment whose original input came from the direct acquisition family, an
operator may declare `RADAR_INGEST_UPSTREAM_DOMAINS=unidata-level2-aws`. This describes the
declared input provenance; it does not turn replay into a live backup. Without a declaration,
leave the setting blank. Do not invent a distinct ID merely to make transports appear independent.
The Compose file passes this setting through without asserting a default dependency.

Example response from such a replay deployment:

```json
{
  "schema_version": 1,
  "input_mode": "replay",
  "failure_domains": ["unidata-level2-aws"]
}
```

The binary reports `replay` when the replay adapter is configured and `idle` otherwise.
Configuring LDM settings does not produce a live declaration: no live LDM adapter is implemented
in this binary. Replay and idle modes disclose that the relay is not a live upstream backup.
The library router retains its existing constructor and defaults to unknown; applications with
an implemented acquisition adapter can use `router_with_declaration` to provide its declaration.

The native monitor refreshes metadata before each subscription attempt. Requests and bodies
are bounded to two seconds and 16 KiB, with no redirects. The monitor also bounds provider
inspection to two seconds and observes cancellation every 250 ms. A failed optional lookup
still permits the data subscription. Bare `host:port`, HTTP and HTTPS configuration follow
the existing relay transport conventions. Old servers returning 404 remain usable with unknown
metadata. Unsupported schema versions, invalid IDs, oversize bodies and timeouts yield fixed
unavailable reasons; endpoint addresses and untrusted response/error bodies do not enter
topology diagnostics. No changes are made to canonical radial identity, acquisition receipts,
subscription framing, continuation admission, failover selection or detector inputs.

## Evidence and limits

The [verification manifest](upstream-domains/verification.json) records source and log hashes, deterministic declaration controls,
real loopback HTTP exercises, monitor cancellation/timeout controls, production diagnostics
serialization, and narrow Sources captures. The current/previous distinction is exercised at
240 and 300 px using the production source-detail renderer and actual failover detail builder.
These checks establish this disclosure contract, not operational redundancy.

Final Windows verification passed 2,228 workspace tests across 28 suites, with zero failures and
144 explicit ignores; strict workspace/all-target Clippy and WASM compilation also passed.
The browser's existing warnings remain (wxdata 1, hookecho 10). All ten new controls passed.
The explicitly invoked GPU control passed in 4.47 seconds, producing eight inspected captures
at 240/300 px with no horizontal clipping. The manifest hashes fourteen source files and all
logs/captures. Four exported source-health rows retain the exact production serialization:
[shared declaration](upstream-domains/shared-diagnostics.json),
[different declarations](upstream-domains/distinct-diagnostics.json),
[unknown declaration](upstream-domains/unknown-diagnostics.json), and
[failed refresh with dated history](upstream-domains/refresh-failed-diagnostics.json).
These are synthetic health fixtures with fixed clocks, not observations from an operational session.

A relay HTTP declaration is endpoint configuration evidence. It is not authenticated upstream
attestation, bound to a WebSocket stream epoch, or a topology audit. A reverse proxy could route
metadata and radial requests to different processes. Shared radar origin, networks, power,
providers and acquisition infrastructure can remain common even when domain IDs differ.
Metadata is refreshed on reconnect, not continuously during a healthy stream. Failover behavior
continues to use observed data health; this increment does not require declarations to deliver
radar data or reject replay fixtures used for testing.

M1.3 remains partial. Full application delivery/rendering fault sessions, cross-provider cut
continuation, independently verified topology, operational direct/relay sessions under sustained
load, cadence calibration and applicable platform/device gates still require evidence.
Tornado detection methodology remains with Claude.
