# M3.2 unit and datum typing (1008.md C4)

User-defined product formulas are checked for the physical quantities they combine
(`wxdata::udp::Expr::unit_diagnostics`, `crates/wxdata/src/udp/units.rs`). Every input has a
quantity — dBZ, dB, m/s, °/km, a ratio (CC), km, degrees, and heights in metres above the antenna
(`BEAM_HEIGHT_M`, every column function's height) or above sea level (`BEAM_ALTITUDE_M` and the
isotherms). The editor notes, in amber, each place a formula adds, subtracts, compares, takes the
min/max/mean of, or chooses between two different quantities; each place it mixes the two height
datums, including an isotherm (MSL) used as a column layer bound (read above the antenna); and a
units label naming a different quantity than the formula computes. A quantity scaled by a
coefficient is how an index weighs its terms, so it is not checked further. Notes are advice: a
formula that does not parse is still the red error that disables "Show on map".

The 32 reference products in `docs/presets/radar-analyst-custom-products.json` import with nothing
refused or adjusted, each evaluates to a finite value on at least one synthetic storm gate or
column, and typing finds nothing to note in any of them (`crates/wxdata/tests/udp_presets.rs`).

The sounding height source for archived volumes before HRRR already existed before this change:
an archived volume's isotherms come from that day's observed ascent, labelled with the station and
launch (`app/env_levels.rs`, `EnvLevels::observed`).

## Reviewed capture

`cargo test -p hookecho --lib -- --ignored gpu_unit_notes_snapshot` (RTX 2060, offscreen) writes
it to `target/parity-review/m3.2/`.

| File | What | SHA-256 |
| --- | --- | --- |
| [unit-notes.png](unit-notes.png) | A unit mix, a datum mix, a mislabelled unit, and a formula that does not parse | `d6367daf30f4e768dcea183833b5841541ddb443759d4cd7d66ba8f069b182b9` |

## Not established

- Notes are not yet shown on import from a file (the import diagnostics list only refusals and
  adjustments), nor in the gate inspector.
- No Android editor run.
