# M4.3 symbols by attribute (1008.md D2)

An imported layer can give each value of an attribute its own point symbol ("Symbol by" in the
Layer Manager), alongside colouring by the same or another attribute. The commonest values get
the circle, square, triangle, diamond and cross in that order (ties by value); further values and
features without the attribute keep the layer's own symbol, and the legend says how many share
it. Numbers are treated as categories: a symbol is a kind, not a quantity. Stored as
`symbol_by` on the layer, omitted when off, so older settings load unchanged.

| File | What | SHA-256 |
| --- | --- | --- |
| [symbol-legend.png](symbol-legend.png) | The legend for five values and two more (RTX 2060, `gpu_symbol_legend_snapshot`) | `a6df060f1e9ac6b7849640f11452a2db09b4cc4084f8eecbdde98c00416d49de` |

Tests: `values_get_their_own_symbols_commonest_first`, the settings round trip with `symbol_by`.

Not established: a dense asset layer drawn with mixed symbols on the map, label priority beyond
paint order, and the Android chooser/filter walkthrough.
