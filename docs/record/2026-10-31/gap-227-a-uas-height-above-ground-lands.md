# GAP-227: a UAS height above ground lands on a staged store

GAP-199's ground model (`gungnir-app/src/ground.rs`, D-130) places a Category 129 UAS that
reports only its height above ground. Before this change it read one placed DEM. When a
baseline named a staged store (`terrain.store`, DN-36), it refused by name. It now reads the
store.

**Which surface, and why.** There were two candidates. One was a lookup of its own over the
store's tiles. The other was the `HeightField` line of sight already paints from the pinned
store over the coverage extent (DN-36 §5.2). This change takes the field, for four reasons.
First, the field is read from the manifest-verified tiles alone and never from the fill
cache, so D-164 holds without a second rule to keep. Second, it is painted on the tile
worker once per extent, so the feed's adapter reads a raster on the gateway's thread and
never opens, verifies or places a tile file there. A tile lookup would have needed its own
cache, its own eviction and its own thread. Third, the UAS sits on the same ground line of
sight masks against, as it did on one DEM (GAP-247). Fourth, the extent is every declared
sensor's range and approach with one maximum range to spare, which is where the picture's
sensors can see a UAS.

What that costs. The field is painted at the finest level that fits
`HEIGHT_FIELD_CELL_BUDGET`, which over a wide extent is coarser than level 0. Between cell
centres it is bilinear. The report's terrain name says which level placed it
(`<store> at level <n>`). The vertical error rule is unchanged: `terrain.vertical_sigma_m`
is added to the up variance exactly as for one DEM. Nothing is added for the smoothing at a
coarser level, so on a coarse level over steep ground the stated sigma understates the
error. A Category 129 gateway whose own coverage is not a declared sensor can report a UAS
past the extent, and that UAS is unknown rather than placed.

**Where the ground does not reach.** The field holds no data past the store's edge and in
cells of a refused tile. Past the extent there is no field at all. A position there has no
ground height: the report keeps the height unknown at 0 with D-130's widened variance. The
reason names the edge. Past the extent is told from the field's own bounds. Past the store's
edge is told through the store's placed footprint (`StoreFootprint::frame_of` against the
manifest's extent), which `TileStreaming::footprint` now exposes. Anything else is a cell
inside the store with no height.

**When it is lent.** `lendable` refuses while the field is building, and with the reason
when it is not built (for example, no sensor declared to cover). It lends once it is built.
`lend_to_feeds` used to act only when `TerrainStatus` changed. A field painted again over a
moved extent can leave the status equal, so the state now also keeps the field it lent and
compares it by pointer (`Arc::ptr_eq`). Comparing cell for cell every tick would cost what
the check exists to save. The model shares the desktop's `Arc`, since a painted field is
never written again.

**The tests.** `gungnir-app/tests/uas_above_ground.rs` stages a 64 by 64 store in the local
frame, rising 0.1 m per metre east from 100 m at the origin, and drives the desktop's own
tick:

- `a_height_above_ground_lands_on_a_staged_store`: the ingest fixture's record lands at
  100 m plus its 120 m, with the stated 2.5 m in its up variance, on a name ending
  `at level 0`. A half-cell misplacement would miss by 0.5 m against a 0.01 m tolerance.
  The same field raised 10 m and put in the state is lent anew, and the record lands 10 m
  higher.
- `past_the_stores_edge_or_the_coverage_extent_a_height_above_ground_is_unknown`: a store
  ending 60 m south-west of the origin, inside the extent, gives "past the edge of". A
  1.9 km store with the radar about 640 m north gives "past the coverage extent". Both
  leave the height unknown, and PN-03 marks the track.
- `a_store_with_no_coverage_extent_lends_no_ground`: with no sensor declared, nothing is
  lent, and the reason carries line of sight's own.

**Not done.** The node still reads no terrain, so every such height on it stays unknown
(D-130), and DN-36 §9's node reading the same store is a separate task. Nothing here was
looked at on a display.
