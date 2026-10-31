# GAP-227: line of sight from the pinned store

The third increment of `docs/design/DN-36-staged-terrain-and-map-tiles.md`, §5.2 with D-164
(amendment 2). A staged store is now what line of sight, the dead spots and the coverage
claims read. They read it over the coverage extent and never over the view.

**The extent.** `terrain_tiles::coverage_extent` takes every declared sensor's position in
the local frame, a box of its maximum range about it, and every approach point, and widens
the union by one maximum range. With no origin, or with nothing declared to cover, it says
which, and the terrain line carries that reason with line of sight flat. It is read from the
baseline on every tick and compared with the extent last asked for, so a field is painted
again only when the sensors or the approaches move. Panning, zooming and tiles streaming in
never repaint it.

**The field.** The tile worker paints it, off the render thread. It picks the finest level
whose cells over the extent keep the raster within `HEIGHT_FIELD_CELL_BUDGET`, or, when none
does, the coarsest level at a cell made coarser to fit. It then reads, verifies and places
each held tile of that level under the extent, paints it into a blank raster, and drops it.
`HeightField::resample` became `HeightField::blank` plus `HeightField::paint`, with the same
arithmetic. `painting_in_pieces_is_resampling_whole` holds that a surface painted in two
overlapping pieces, as tiles are with their shared apron, gives the whole resample cell for
cell, and the analytics suite is otherwise unchanged. A tile refused there, and the extent
past the store's edge, stay no data: flat, and the line says how many and that the extent
reaches past the edge. The pinned store is the only source the worker reads, so a fetched
fill tile, when there is one, cannot enter the field (D-164).

**The status.** `TerrainStatus::StoreVerified` carries a `SightState`: building, built (the
level, the tiles painted, those refused, whether the extent passes the edge), or not built
with why. `is_masking` is true only when built. The dead spots read the field then, through
the same arm as a single DEM's.

**The test, §8.1 row f (a draft).** `line_of_sight_reads_the_coverage_extent_and_ignores_the_view`
declares one radar at the origin reaching 200 m, against a 640 m store whose south-west
corner is the origin. The field is level 0's 10 m cells over the 800 m extent: ground inside
the store, no data south-west of it, and the line says both. The view then pans east, north
and out, and the tiles that stream in are drawn. After each move the field is the same
allocation (`Arc::ptr_eq`), not an equal copy, so nothing was painted again. The dead spots
are computed from that field alone (DN-34 §6.1), so they cannot have moved either. The test
does not compute a dead-spot field itself.

**Not done.** The UAS ground model (GAP-199) still reads one placed DEM, and with a store it
says so by name rather than placing a height on ground it has not read. The node reads no
terrain at all, so DN-36 §9's "the node reads the same store for its line of sight" is still
to build. Row k waits on the fill cache, which is the next increment. Nothing here was looked
at on a display.
