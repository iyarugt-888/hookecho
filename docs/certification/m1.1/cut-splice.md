# Cross-provider cut continuation decisions (1008.md A5)

`wxdata::continuation::CutSplice` is the single decision a live volume assembly applies to a
block offered by any provider. It combines the module's existing pieces:

- the exact-`VolumeKey` continuation rule (`check_volume_continuation`);
- volume rollover;
- radial deduplication by `(volume, cut with repeat index, azimuth number)` (`RadialDedup`).

Before this, those pieces existed but nothing used them together, and nothing outside the
module used them at all.

| Offered block | Outcome |
| --- | --- |
| The first block, or a newer volume (a VCP change starts one) | `NewVolume`: the assembly resets there and these radials begin it |
| The same volume | `Accept { new, duplicates }`: only radials not yet accepted, from this or another provider; the rest are counted, never drawn twice |
| An older volume | `Refused(OlderVolume)`: never mixed into the newer one |
| Another radar | `Refused(ForeignRadar)` |
| No radial identity (metadata) | `PassThrough` |
| A radial span that cannot be listed (wraps past 0° within the block) | `Unidentified`: a splice refuses it rather than risk drawing radials twice |

## Controls (`cargo test -p wxdata --lib continuation`)

- `a_mid_volume_join_adds_only_what_is_new`: the primary sends azimuths 1–200, then a backup
  joins with 150–260. Only 201–260 are added, and 51 are counted as duplicates.
- `gap_fill_and_reordering_draw_each_radial_once`: the backup fills 101–140, which the primary
  missed. Out-of-order repeats from either provider add nothing.
- `repeated_cuts_are_their_own_radials`: a SAILS revisit (repeat 1) of a fully seen base tilt is
  all new, and its own repeats are duplicates. An MRLE revisit (repeat 2) is new again.
- `a_vcp_change_rolls_over_and_late_old_radials_are_refused`: a new volume start resets the
  assembly, and a late radial from the old volume is refused. Another radar is refused. A start
  differing by 400 ms is the same volume, because `VolumeKey` keeps whole seconds.
- `unlisted_radials_are_not_spliced`: metadata passes through, and a wrapped span is
  `Unidentified`.

## Not established

The app's live assembly does not call `CutSplice` yet. A provider switch still starts a fresh
subscription, and its first volume admission is decided by the existing `SourceVolumeCursor` and
`LiveScan::accept_volume`.

Wiring the splice into the switch is part of A2's failover under load, together with operational
verification on real dual feeds. That work must keep receipts immutable and keep message holes
distinct from proven loss.
