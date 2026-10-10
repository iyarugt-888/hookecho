# M1.3 — Cross-provider continuation over the relay wire (1008.md A2)

Date: 2026-10-10. Builds on [subscription ownership](live-session.md) and
[restoration](restoration.md).

## What a switch does

When a pane's provider changes, `radar_feed` starts the new stream with the scan the pane is
showing as its base (`spawn_stream(idx, site, base, …)`). Every provider merges what it receives
into that base through `wxdata::live::merge_scan`. This is the same rule the primary uses for its
own chunks:

- At each azimuth of a tilt, the newest radial wins, so a radial both feeds delivered is drawn
  once.
- Radials more than 15 minutes (`RETAIN_MS`) older than the newest are pruned.
- A VCP change replaces the scan as a whole.

The library-level `continuation::CutSplice` (A5) is not called by the app. The switch relies on
the merge above.

## The control

`relay_provider::integration_tests::a_backup_continues_the_primarys_volume_without_duplicating_radials`
runs two independent in-process relays (`radar_ingest::server::router`), each over a real
WebSocket on loopback, with one radial per block:

1. **Primary:** the volume's VCP and its first two radials at tilt 1, then nothing more. The pane's
   scan holds azimuths 1 and 2.
2. **Backup:** started with that scan as its base, on a second relay serving the whole volume,
   including the two radials the primary already sent. Its final scan holds tilt 1 azimuths
   1, 2, 3 and 4, each exactly once, and tilt 2 azimuth 1. Nothing is duplicated and nothing is
   lost.
3. **Lagging backup:** a third relay serves a volume 20 minutes older than the shown scan. It
   sends no update at all: its radials are pruned against the newer base, the merge reports
   nothing changed (`relay_provider.rs`, `changed.is_empty()`), and the display is not rewound.
4. **Stale scan:** a shown scan holding a radial from a volume 20 minutes older (azimuth 9). A
   stream of the current volume prunes it from the tilt, so it is not mixed into the new volume.

Step 3 was found while writing the control. Its first draft expected the older volume to arrive,
and the stream sent nothing.

## Not established

- This is a loopback control with synthetic radials, not an operational dual-feed failover.
- No interactive session has switched providers mid-volume while drawing.
- Failure-domain identity of real upstreams, sustained load, and platform gates are still open,
  as listed in the card.
