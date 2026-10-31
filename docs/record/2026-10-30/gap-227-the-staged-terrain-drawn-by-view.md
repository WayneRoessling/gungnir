# GAP-227: the staged terrain drawn by view

The second increment of `docs/design/DN-36-staged-terrain-and-map-tiles.md`: §5.1, §5.3 and
§6. A verified store's tiles are chosen by the view, placed off the render thread, and drawn
by both renderers. Line of sight over the store is the next increment; until it lands the
terrain line still says line of sight is flat.

**Choosing tiles, `gungnir_viewport3d::streaming::terrain`.** `StoreFootprint` holds every
tile's box in local ENU, read from the level-0 tile corners, which the desktop places once
per store through the same placement as the tiles. `tiles_in_view` tries each level, finds
the smallest power-of-two stride at which the tiles touching the view fit
`TERRAIN_VERTEX_BUDGET`, and takes the finest level whose drawn cells are at least four
points across. It is pure. `tiles_in_view_cover_the_view_once_within_the_budget` zooms a
600 by 400 cell store from 1.25 to 160 m a point at three centres. At each step it checks
that every visible point of the store lies in a chosen tile, that no two chosen tiles
overlap, that the vertices stay within the budget, and that "past the edge" is said exactly
when the view leaves the store (§8.1 row c, a draft).

**Drawing them.** `tile_surface` builds one tile, at one stride, out to its far apron. It
colours from the file's heights over the manifest's range, and shades from one-cell
differences, taken toward the north and west at the edges two tiles share.
`a_shared_edge_is_one_edge_in_both_tiles` checks at strides 1 to 8, east and south, that
every shared vertex has the same position and colour in both tiles and that there are as many
as the stride leaves (§8.1 row d). The tiles drawn are joined into one surface with a
revision of its own, which the projection's mesh and the scene's ground pass draw as they
draw the single DEM. They are joined again only when the set drawn changes. The single DEM's
`terrain_surface` is untouched.

**Missing ground.** A wanted tile not yet resident is stood in for by its nearest resident
ancestor, which replaces its siblings. `TileReport` counts what is wanted, drawn, standing
in, loading and refused, and whether part of the view has no staged ground. The map draws it
as one sentence ("no staged terrain past the store's edge", "tile 0/1_1 refused: digest:
...", "1 terrain tile loading, coarser ground stands in"), and PN-09's terrain line counts
the tiles placed, loading and refused. `a_missing_tile_is_stood_in_for_and_said` covers the
stand-in and the refusal, and checks that an unchanged frame joins nothing.

**The desktop, `gungnir_app::terrain_tiles`.** When a store is verified, a worker thread is
started. It places the footprint and then each tile the last frame wanted: read and verified
against the manifest, meshed, and placed through `terrain::place_tile`. At most 8 are in
flight and 24 kept, the least recently wanted dropped first. A refused tile is not asked for
again. `tests/terrain_tiles.rs` draws a store through the real projection
(`picture::render_projection` under `RenderProbe`). Inside the store it finds level 0 at
stride 1 under the budget, with nothing said missing. Zoomed out it finds a coarser level and
the sentence for the store's edge, on the map and on PN-09. A tile tampered after staging is
said by its key while the rest is drawn (§8.1 row g).

**What the build changed in the design** is DN-36 amendment 4: tiles chosen from a placed
footprint, one level and stride per frame, one joined surface instead of a GL mesh per tile,
the ramp read from the file's heights, and a stand-in replacing its children.

**Not checked here.** Nothing in this change was looked at on a display; DN-36 §8.2's checks
are the owner's. A real store has not been staged: the Oregon clip still loads as one DEM,
and `terrain stage` is a later increment. In a real CRS the two copies of a shared vertex are
placed from different tile origins and can differ by under a millimetre; only the local frame
was tested bit for bit.
