# DN-36 Staged terrain and map tiles

Part of GAP-227: its action's last item, "basemap tiles as the first consumer of the
streaming stubs", widened to the ground under the picture as well as the imagery over it.
Status: design, 2026-10-30, built in increments: GAP-227's history says which have landed,
and amendment 3 what the first changed. Four choices were raised for the owner as
D-160 to D-163 (§10), each with a recommended default; the owner took all four the same day,
D-160 and D-162 past the default, and amendment 1 says what they change and raises D-164,
which the owner took to its default (amendment 2). What the owner has signed of this
note is in [`../signatures.md`](../signatures.md).

## 1. Why now, and what it is for

The owner looked at the Oregon terrain (GAP-252, `DN-35-terrain-in-the-three-d-scene.md`)
and zoomed out: the ground stops at the edge of the one clip the baseline names, and nothing
beyond it ever loads. The owner asked whether the desktop should download the surrounding
ground as the view moves, or whether it should be staged beforehand.

`../rust-3d-data-ecosystem-build-vs-adopt.md` §4 deferred tile streaming "until a specific
panel actually requires planet/site-scale streamed terrain". That requirement is now in hand,
for PN-02, the picture. This note is the design §2.1 of that document sketched, made concrete
for this system: where tiles come from, what form they are staged in, how the picture chooses
which to draw, and what line of sight reads. Imagery, GAP-227's basemap, comes after the
ground and uses the same machinery (§7). GAP-227's other items are untouched by this note:
D-139's MIL-STD-2525D symbols, the cursor readout and the declutter.

## 2. What exists

Read from the code at `cabb9b43`:

| What | Where | What it gives this note |
|---|---|---|
| One DEM per baseline | `gungnir-config::TerrainConfig` (one `path`); `gungnir-app/src/terrain.rs::start` sends one `LoadRequest::Terrain`; `layer` reads `data.terrains.first()` | The single-file model this note extends. A baseline naming one file keeps working unchanged |
| Placement | `terrain.rs::place`: the file's own CRS reconciled with `frame`, heights made WGS 84 ellipsoidal through a verified geoid grid (GAP-108, GAP-197), NAD83 carried by its realisation (GAP-200), then local ENU | Every staged tile goes through this path, unchanged. It is the part of the terrain pipeline with independent oracles behind it |
| Integrity of fetched data | `deploy/geoid/SHA256SUMS`; `gungnir_data::geoid::GeoidGrid::verify(path, sha256)`; the start-up check refuses a grid whose digest differs | The pattern for staged tiles: fetched once, pinned by digest, verified before use, refused by name otherwise |
| The surface both renderers draw | `layers::terrain_surface` (GAP-252): decimated to `TERRAIN_VERTEX_BUDGET` (16 384), coloured by height over the **full grid's** lowest and highest, hillshaded on the decimated grid | Built per DEM. With tiles, the colour ramp and the hillshade's neighbours cross tile edges (§6) |
| Line of sight | `gungnir_analytics::HeightField`, resampled once from the placed mesh under `HEIGHT_FIELD_CELL_BUDGET` (`terrain.rs::height_field_of`) | Reads the one DEM. With a store it needs an extent of its own (§5) |
| Streaming | `gungnir-viewport3d/src/streaming/`: `StreamingLayer::update`, `sse::screen_space_error`, `tileset::parse_tileset_json`, all `NotImplemented` (`../unbuilt.md`); `tile_cache::TileCache` empty | The module boundary §2.1 laid out. This note fills it for terrain first |
| The deployments | `ARCHITECTURE.md` §8: disconnected desktops, on-prem nodes, cloud nodes | A disconnected desktop has no network to fetch from (§3) |

## 3. Where the ground comes from: staged, not fetched (D-160)

**Recommended: a terrain store staged before deployment, which the desktop and the node read
and never write.**

- **A disconnected desktop is the profile the picture has to serve first.** Ground that
  appears only when a network is present shows an operator relief in one direction and
  none in another, for a reason that is not on the screen. A staged store shows the same
  ground in every profile.
- **It is the integrity model the system already has.** The geoid grids are fetched once,
  pinned in `deploy/geoid/SHA256SUMS`, and verified at start. Line of sight, and through it
  the dead spots and every coverage claim, reads terrain. Terrain arriving at run time from a
  service is a coverage claim resting on data nobody pinned.
- **Fetching at run time is a network dependency in the binary.** It needs an HTTP client, a
  §2.9 row, a decision on which service to trust, and a refusal path when the network fails.
  None of that exists, and the store needs none of it in the binary (§4).

The alternatives: fetch at run time from a service named in the baseline, with the store as
a cache; or both, staged first and fetched to fill gaps when connected. Either can follow the
store later, because the reader is the same. The store is what a fetch would fill.

## 4. The store

### 4.1 What is staged

A **terrain store** is a directory holding a manifest and a pyramid of DEM tiles:

```
store/
  manifest.json
  0/<column>_<row>.tif    level 0, the source's own cells
  1/<column>_<row>.tif    level 1, each cell the mean of 2 x 2 of level 0's
  ...
  n/...                   the coarsest level, one tile or a few
```

- **Each tile is a GeoTIFF the existing loader reads** (D-161): Float32, DEFLATE, the
  floating-point predictor, in the source's own CRS and vertical system. So a tile is placed
  by `terrain.rs::place` exactly as the Oregon clip is today. No new decoder and no new CRS
  code, and the oracles behind the placement hold for every tile.
- **A fixed tile size**, 512 by 512 cells, with **one apron cell on every side**, shared with
  the neighbour. The apron is read for the hillshade's normals and for seams (§6), and not
  drawn.
- **Levels by averaging**, never by interpolation: a level-k cell is the mean of the
  2^k by 2^k level-0 cells under it, with a no-data cell left out and a cell with none kept
  as no data. Averaging is the decimation the drawing already does, done once at staging time.

### 4.2 The manifest

The manifest says everything a tile does not:

- the source (URL, retrieval date, licence, as `testdata/dem/SOURCE.md` records for the
  clip), its CRS, its vertical system and its NAD83 realisation where it has one;
- the extent, the level count and, per tile, its level, column, row, bounds and **SHA-256**;
- the **lowest and highest height over the whole store**, so every tile is coloured on one
  ramp (§6).

The baseline names the store and **pins the manifest's digest**: `terrain.store` with
`manifest_sha256`, beside the existing `frame`, `vertical` and `nad83_realisation`, which
apply to every tile. A baseline names either `path` or `store`, never both, and validation
says so. A baseline verified against a pinned baseline key (`GUNGNIR_BASELINE_KEY`) carries
the manifest's digest with it, so the chain runs from the verified baseline to the manifest to
each tile. A tile whose digest differs
from its manifest entry is refused by name, and its area is drawn as missing (§5.3).

### 4.3 Who stages it (D-162)

**Recommended: a staging tool outside the binary**, `deploy/terrain/stage.py`, run once on a
connected machine. It fetches the source tiles for a bounding box, verifies each against the
source's published checksum where one exists, cuts and averages the pyramid, and writes the
manifest. It uses the same tools the committed clips were cut with (GDAL, through the pinned
OSGeo image), so its output can be reproduced. `deploy/README.md` gains the procedure, beside
"Pinning a further geoid grid". The binary only reads and verifies.

The alternative is a `gungnir-app terrain stage` subcommand. It would put the network client
and the source-specific code in the binary, which D-160's default exists to keep out.

## 5. What reads the store

### 5.1 The picture: tiles by view

A pure function chooses the tiles a frame wants:

```rust
/// The tiles to draw for this view: for each part of the visible rectangle, the finest
/// level whose cells are at least `MIN_CELL_POINTS` across on screen, within
/// `TERRAIN_VERTEX_BUDGET` over the whole frame. Pure, so the choice is tested headless.
pub fn tiles_in_view(store: &StoreIndex, view: &TopDownView, rect: egui::Rect) -> Vec<TileKey>;
```

This is §2.1's screen-space error for the straight-down camera: a cell's size on screen is
its size in metres over `meters_per_px`, so `sse::screen_space_error` has a closed form here.
The tilt (DN-34 §13.1 row 3) gives it a perspective term later. The budget stays the
projection's 16 384 vertices **for the frame**, not per tile, so zooming out never draws more
than the single DEM does today.

A tile not resident is requested from the loader thread, placed off the render thread, and
built into a `TerrainSurface` once (`rust-ui-architecture-coding-standards.md` §5). Until it
arrives, its parent level is drawn if resident, or nothing (§5.3). The surfaces are held in
an LRU cache keyed by `TileKey`. In the scene, `tile_cache::TileCache` holds the matching GL
meshes. A tile's surface carries its key as DN-35's revision, so the ground pass and
`ground_source` work per tile as they do for the one DEM.

### 5.2 Line of sight: the sector, never the view

**Line of sight does not depend on where the operator has scrolled.** Its height field is
built from the store over the **coverage extent**: the union of the declared sensors' ranges
and the approaches, with a margin of one maximum range. It uses the finest level that keeps
the field under `HEIGHT_FIELD_CELL_BUDGET`. It is rebuilt when the baseline's sensors or the
store change, not when the view moves. Dead spots, gap reports and laydown comparisons all
read the same surface, as they do today.

Ground outside the coverage extent is drawn for context and enters no coverage claim. That
is stated on PN-09's terrain line.

### 5.3 Where the store ends

Beyond the store, and over a tile refused or not yet loaded, **nothing is drawn, and the
viewport says why**: a sentence like the dead-spot layer's ("no staged terrain past the
store's edge", "tile 1/3_2 refused: digest"), and PN-09 counts the tiles resident, refused
and missing. A grey area with no word is the failure DN-12 §7 names: a hidden layer and an
empty one look alike.

## 6. Seams

Three things change at a tile edge, and each has an answer:

1. **The colour ramp.** `terrain_surface` colours over the lowest and highest height of the
   grid it is given. Per tile, the same height would take two colours on either side of an
   edge. Every tile is coloured over the **manifest's** store-wide range instead.
   `terrain_surface` takes the range as an argument, and the single-DEM path passes the grid's
   own, so its output is unchanged.
2. **The hillshade.** A normal at a tile's edge vertex needs its neighbour across the edge. The
   apron cell gives it one, so the shade is continuous. Without the apron, the edge would be
   shaded one-sided and show as a line.
3. **Cracks between levels.** Straight down, a fine tile beside a coarse one leaves no crack:
   the orthographic camera ignores height (DN-35 §4 item 1), and the tiles share their edge in
   plan. At the tilt the heights part, and skirts, a strip hanging down from each edge, are
   that increment's to add.

## 7. Imagery, after the ground

GAP-227's basemap is imagery: a raster drawn over the ground. With the store in place it is a
second pyramid in the same layout, `imagery/<level>/<column>_<row>.<image>`, staged by the
same tool under the same manifest rules. It is chosen by the same `tiles_in_view` and drawn as
a texture: on the ground's mesh in the scene, and as an `egui` image in the projection, as the
dead-spot field is. Its source and format need a decision of their own when that increment is
designed. It follows the terrain because a drape needs a ground to lie on.

## 8. What can be tested headlessly, and what only the owner can see

### 8.1 Headless

Drafted as rows for `../verification-capability-table.md` §2. A pass criterion is human-owned;
these are drafts until the owner agrees them, and are not gates.

| Row | Test | Criterion |
|---|---|---|
| a. The store is verified | `gungnir-app` integration | A manifest whose digest differs from the baseline's is refused by name; a tile whose digest differs from its manifest entry is refused by name, and the rest of the store loads |
| b. Levels average | the staging tool's own check, and a `gungnir-data` test over a committed two-level store | Each level-1 cell is the mean of its four level-0 cells, no-data left out |
| c. Tiles by view | `tiles_in_view` unit tests | Across zooms from the store's finest cell to its whole extent, the chosen tiles cover the visible rectangle, no two overlap, and their vertices stay within the budget |
| d. One ramp, no seam | `terrain_surface` with the store's range | A height takes one colour in every tile; a vertex on a shared edge has the same position, colour and shade in both tiles |
| e. A single DEM is unchanged | `the_surface_is_the_projections_ground` (GAP-252) | Passing the grid's own range reproduces today's surface exactly |
| f. Line of sight ignores the view | `gungnir-app` | The height field and the dead-spot field are identical before and after the view pans across the store |
| g. Missing ground is said | `RenderProbe` | With a tile refused or the view past the store, the viewport and PN-09 say so |

### 8.2 Only on a display

1. Zooming out from the Oregon sector, the ground continues past the clip's old edge, with no
   seam in colour or shade at a tile edge.
2. Zooming in, the relief sharpens, and no tile pops visibly when its finer level arrives.
3. Past the store's edge, the sentence says so.
4. Every check in DN-35 §8.2 still passes.

## 9. Interface delta

- `gungnir-config`: `TerrainConfig` gains `store` and `manifest_sha256`, exclusive with `path`.
- `gungnir-data`: a store index and manifest reader. Tiles are read by the existing GeoTIFF
  loader.
- `gungnir-app`: `terrain.rs` loads tiles through the loader thread and places each through
  `place`. The height field is built over the coverage extent. PN-09 counts tiles.
- `gungnir-viewport3d`: `tiles_in_view`; `terrain_surface` takes a height range; the
  `streaming/` stubs filled for terrain tiles; a surface cache.
- `deploy/terrain/stage.py` and its procedure in `deploy/README.md`.

**No dependency edge, and no new crate in the binaries** under D-160 to D-162's defaults: the
store is GeoTIFF, read by the existing `tiff` and `proj` paths. A runtime fetch or a 3D Tiles
format would each need a §2.9 decision. The node reads the same store for its line of sight.

## 10. The decisions

| Id | Choice | Recommended default | The alternatives |
|---|---|---|---|
| **D-160** | Where ground beyond the named DEM comes from | **Staged before deployment**: a store the desktop and node read and never write, its manifest's digest pinned in the baseline (§3) | Fetched at run time from a service the baseline names, with the store as a cache; or staged and fetched to fill gaps when connected |
| **D-161** | The store's tile format | **A pyramid of GeoTIFF DEM tiles** in the source's own CRS, read and placed by the path GAP-023, GAP-108, GAP-197 and GAP-200 built (§4.1) | Quantized-mesh or 3D Tiles terrain, which brings §2.1's 3D Tiles dependency forward and needs a placement path of its own |
| **D-162** | Who stages the store | **A tool outside the binary**, `deploy/terrain/stage.py`, using the pinned GDAL image (§4.3) | A `gungnir-app terrain stage` subcommand, putting the network client in the binary |
| **D-163** | The order of GAP-227's parts | **Staged terrain first** (this note), since it is the requirement in hand and imagery drapes on it; the cursor readout, the declutter and D-139's symbols proceed independently in parallel | Symbology and readout first, the tiles after |

## Amendment 1, 2026-10-30: the owner's answers (D-160 to D-163)

| Id | Taken | What it changes |
|---|---|---|
| D-160 | **Past the default: staged first, and fetched to fill gaps when connected** | §3 and §5.3 (item 1 below) |
| D-161 | The default: a pyramid of GeoTIFF DEM tiles | Nothing. §4.1 stands, and fill tiles take the same form |
| D-162 | **Past the default: a `terrain stage` subcommand in the binaries** | §4.3 (item 2 below) |
| D-163 | The default: the staged terrain first, GAP-227's other parts in parallel | Nothing |

1. **Fill when connected (D-160).** The pinned store is unchanged, and it is still the only
   ground a disconnected deployment has. A deployment whose baseline names a fill source
   (`terrain.fill_source`, absent by default) fetches the tiles its view or its coverage extent
   needs and its store lacks:
   - **into a fill cache under its own data directory, never into the pinned store.** The
     manifest the baseline pins never changes at run time;
   - **on the loader thread, never the render thread**, as the source's own tiles, cut and
     averaged into the same pyramid form by the code `terrain stage` uses (item 2), and placed
     by the same path;
   - **with its digest recorded in the cache at fetch time.** That records what was fetched,
     not that anybody vouched for it. Whether such a tile enters line of sight, and so the dead
     spots and the coverage claims, is **D-164**, raised with this amendment;
   - **marked.** A fill tile is drawn with a distinct edge and counted apart on PN-09
     ("pinned 42 tiles, fetched 6, refused 0"). A fetch that fails, or a desktop without a
     connection, is said in the same place, and the area is drawn as missing (§5.3). The
     picture never waits on a fetch.

   The fetch needs an HTTP client with TLS. Its crate is a §2.9 decision for the build, which
   that pull request states with its `Decision:` and `Duplicate linkage:` lines. It sits
   behind a feature of its own, off in a build that must never reach a network, as `crs` is.
   §9's "no new crate in the binaries" no longer holds.

2. **`terrain stage` in the binaries (D-162).** `gungnir-app terrain stage` and `gungnir-node
   terrain stage`, one implementation, as `account` is. Given a bounding box, a source and an
   output directory, it fetches the source tiles, checks each against the source's published
   checksum where one exists, cuts and averages the pyramid, writes the GeoTIFF tiles and the
   manifest, and prints the manifest's digest for the baseline. The cutting and averaging are
   the workspace's own code, not GDAL's, shared with item 1's fill. So §8.1 row b becomes a
   unit test of that code, and the manifest records the version that staged it.
   `deploy/README.md` still gains the procedure, now as a command line.

3. **§8.1 gains three draft rows:**

   | Row | Test | Criterion |
   |---|---|---|
   | h. No fill without a source or a connection | `gungnir-app` | With no `fill_source`, or with the source unreachable, nothing is fetched, the picture and the coverage claims are those of the pinned store alone, and PN-09 says why |
   | i. The pinned store is never written | `gungnir-app` | After a session that fetched fill tiles, every file under the store matches its manifest digest, and the manifest matches the baseline's |
   | j. A fill tile is marked | `RenderProbe` | A fill tile's edge is drawn in its own style, and PN-09 counts it apart from the pinned tiles |

   §8.2 gains a check: with a fill source and a connection, ground beyond the store appears,
   marked, and the sentence for missing ground does not.

## Amendment 2, 2026-10-30: D-164, a fill tile stays out of line of sight

The owner took D-164 the same day, to the recommended default: **a fetched fill tile enters
line of sight only once it is staged into the pinned store.** Until then it is ground drawn for
context, marked as amendment 1 item 1 says, and nothing more.

- **§5.2 stands as written.** Line of sight's height field is built from the pinned store
  alone, over the coverage extent. Where that extent reaches past the pinned store, line of
  sight there is flat, and the dead-spot layer's "no terrain data" pattern and PN-09 say so,
  as they do past the one DEM today.
- **Pinning a fill tile is staging it.** `terrain stage` can take the fill cache's tiles as
  its source, writing a new manifest whose digest a revised baseline pins. Nothing moves a
  tile from the cache into the store at run time.
- **§8.1 gains row k:** with fill tiles resident over part of the coverage extent, the height
  field and the dead-spot field are identical to those with no fill source at all.

## Amendment 3, 2026-10-30: the store reader, as built

The first increment, §9's `gungnir-config` and `gungnir-data` lines and the placement of
a tile, made four things exact that the note left open, and moved one forward.

1. **The geometry (§4.1).** Tiles are counted from the extent's north-west corner, column
   east and row south, and a level's last column and row of tiles hold what is left. Level
   `k` has the level-0 count halved `k` times and rounded up, so a coarse cell at the east or
   south edge averages what is left and reaches past the extent by less than one of its
   cells. The manifest still lists each tile's shape and bounds (§4.2); the reader derives
   both from these rules and refuses a manifest whose entries disagree, so a choice of
   tiles by view (§5.1) works from geometry that holds.
2. **No predictor (D-161).** `tiff` 0.11 writes neither the floating-point predictor nor
   the horizontal one for floats. A staged tile is Float32 and DEFLATE without one: larger
   than GDAL's `PREDICTOR=3` would make it, read identically. D-161 is otherwise unchanged.
3. **An empty tile is not staged.** A tile whose own cells hold no height is not written and
   the manifest does not list it; its area is missing ground, as past the store's edge (§5.3).
4. **The manifest and the baseline (§4.2).** The manifest records the frame, the vertical
   system and the NAD83 realisation it was staged with. The baseline's frame must be the
   manifest's. A vertical system or a realisation only one of the two states is that one's,
   pinned by the same digest; one both state must be the same. Anything else is refused by
   name before a tile is read.
5. **The cutting and averaging land first.** `terrain stage`'s command line and its fetch are
   a later increment, but the code that cuts a grid into the pyramid and writes the tiles and
   the manifest (amendment 1 item 2) is built now, in `gungnir-data`, because the tests stage
   their stores with it. §8.1 row b is its test, a draft as before. Each level carries the sum
   and the count of the level-0 cells under it, so a level-`k` cell is the mean of those
   cells, as §4.1 says, and never a mean of means.

Until §5's increments land, a verified store is neither drawn nor read by line of sight, and
the terrain line on PN-09 says so.

## Amendment 4, 2026-10-30: the picture's tiles, as built

The second increment, §5.1, §5.3 and §6, changed five things in how the design said it would
draw the store.

1. **Tiles are chosen from a placed footprint, not the manifest alone.** The manifest's
   bounds are in the store's own frame, and the view is in local ENU. So `tiles_in_view`
   takes a `StoreFootprint`: the level-0 tile corners, placed once per store through the
   tiles' own placement, from which every tile's box at every level is read. Its signature is
   `tiles_in_view(footprint, view, rect)`.
2. **One level and one stride for the frame.** A 512-cell tile is 264 196 vertices with its
   apron, sixteen times the frame's budget. So the choice is a level and a power-of-two
   stride together: every pair whose tiles in view fit `TERRAIN_VERTEX_BUDGET`, then the
   finest whose drawn cells are at least `MIN_CELL_POINTS` (4) across. A tile is drawn every
   stride cells out to its far apron, which is the neighbour's first cell, so tiles at one
   level share their edge vertices.
3. **One surface, not a GL mesh per tile.** The tiles drawn are joined into one
   `TerrainSurface` whose revision has its top bit set, so it never equals a single DEM's.
   The projection's mesh and the scene's ground pass draw it unchanged (DN-35 §6). It is
   joined again only when the set drawn changes, and it is under the budget, so such a frame
   uploads at most 16 384 vertices. `tile_cache::TileCache` stays the 3D Tiles stub it was,
   and the per-tile surfaces are kept in the viewport's own `SurfaceCache`. §5.1's "a tile's
   surface carries its key as DN-35's revision" does not apply: the joined surface carries
   its own.
4. **The ramp and the shade (§6).** `terrain_surface` is unchanged, so the single DEM's
   surface is the same bytes as before, and `the_surface_is_the_projections_ground` still
   holds it. A tile is built by `tile_surface` instead, coloured over the manifest's range
   from the file's own heights, not the placed up. The placed up bends away with the earth's
   curvature, by some 200 m at 50 km, and a store is wide enough for that to show. The
   hillshade is taken one cell each way on the level's own grid, whatever the stride, and
   toward the north and west at a tile's first row and column and at its far apron. Two
   tiles hold the same cells there, so a shared vertex has the same colour and shade in both.
   In the local frame its position is the same to the bit. In a real CRS each tile is placed
   from its own origin, so the two copies of a shared vertex can differ by the rounding of an
   `f32` offset, under a millimetre.
5. **A stand-in replaces its children.** A tile not yet resident is stood in for by its
   nearest resident ancestor, and that ancestor replaces every wanted tile under it, so no
   ground is drawn twice at double alpha. With none resident, nothing is drawn there.

The desktop places tiles on a worker thread of its own, keeps at most 24 placed (about four
megabytes each at 512 cells), and asks for at most 8 at once. The sentence for missing ground
is drawn on the map, and PN-09's terrain line counts the tiles placed, loading and refused.

## Amendment 5, 2026-10-31: the fetch (D-173), and the fill source's form (D-174)

**D-173, taken by the owner the same day to the recommended default.** The one HTTP client
a fetch uses is `reqwest`, already §2.9's row and already linked in both binaries, with its
`blocking` feature. It sits behind `gungnir-data`'s `terrain-fetch` feature, off by default,
with `terrain-fetch` features in `gungnir-app` and `gungnir-node` that turn it on. §2.9's row
is widened to say so. Three things follow:

1. **Which roots.** The API client trusts the deployment's own roots through the event
   stream's `rustls::ClientConfig`. A public source such as the USGS is signed by a public
   authority, so the fetch verifies against the operating system's trust store, which is
   what `reqwest`'s `rustls` feature selects (`rustls-platform-verifier`). Only `https://`
   is fetched, or `http://` to the loopback, a mirror on the same machine. Nothing crosses a
   network unauthenticated.
2. **`terrain stage --source <URL>`** fetches the file beside `--out`, checks it against
   `--source-sha256` when one is given, stages from it, removes it, and prints the file's
   own SHA-256 with the manifest's. A build without the feature refuses a URL by name.
3. **The fill cache uses the same client**, so it needs no further crate decision.

**D-174, raised: what a fill source is.** Amendment 1 says a fill tile is "the source's own
tiles, cut and averaged into the same pyramid form by the code `terrain stage` uses", but
names no form for `terrain.fill_source`. The build cannot choose one without the owner.

| | Form of `terrain.fill_source` | What a fetch does |
|---|---|---|
| **Recommended** | A URL template for one-degree geographic source tiles, with `{ns}{lat}{ew}{lon}` naming a tile by its north-west corner, as the USGS 3DEP 1 arc-second product is named (`n44w124`) | For the area the view or the coverage extent wants and the pinned store lacks, fetch each one-degree tile, stage it with `terrain stage`'s code into its own sub-store under the data directory, and record the digest at fetch time. Only a store in a geographic frame can be filled |
| Alternative | The URL of a larger staged store in DN-36's own layout, a mirror of `manifest.json` and its tiles | Fetch that manifest, then the tiles the view wants, verified against the mirror's manifest. No cutting at run time, but a mirror somebody has to stage and serve, and a manifest nobody pinned |
| Alternative | An OGC Web Coverage Service | A request per area. Needs a client for the protocol, and puts the cutting on the server |

Whichever form is taken, the fill is drawn **only where the pinned store has no tile**, with
a distinct edge, and counted apart on PN-09 ("pinned 42, fetched 6"). It is fetched on the
tile worker, never the render thread. It is never written into the pinned store, and it
never enters line of sight (D-164), which the worker's field already guarantees, because the
field reads the pinned store alone. §8.1 rows h to k test it.

## Amendment 6, 2026-10-31: D-174 taken, and the fill cache as built

The owner took D-174 the same day, to the recommended default: one-degree geographic tiles
named `{ns}{lat}{ew}{lon}`. The other two forms are rejected. The fill cache is built on
that answer. Five things are worth stating:

1. **Which tiles.** The fill is fetched for the view, when the view reaches past the pinned
   store's edge. Each corner of the view is mapped back to degrees through the store's
   placed corners (`StoreFootprint::frame_of`, an affine map good enough to name a
   one-degree tile and nothing finer). Of the one-degree tiles the view touches, those
   wholly inside the store are skipped, at most four are taken per view, and they are
   fetched one at a time. No tile is asked for twice in a session, so one that failed is
   said and not retried until the desktop restarts. The coverage extent fetches nothing:
   line of sight never reads fill (D-164), so fill over the extent would be drawn context
   only, and the view already asks for that.
2. **Where it lands.** Each tile goes into `<data_dir>/terrain-fill/<name>/`, staged in the
   pinned store's frame, vertical system, realisation and tile size. Beside it,
   `<name>.fetched.json` records the URL, the source file's SHA-256, the sub-store's manifest
   digest and the time. A later session reopens the sub-store against that recorded digest
   before it fetches anything. A sub-store changed on disk is refused, the tile is said as not
   fetched, and it is not fetched again in that session.
3. **Only where the pinned store has no tile.** A fill tile is drawn with its triangles
   left out wherever their centre lies inside a level-0 tile the pinned store holds. Both
   are coloured on the pinned store's height range. The fill's tiles are chosen within
   what the pinned tiles leave of the frame's budget, and drawn tiles are outlined on the
   map in the warning colour.
4. **Said and counted.** Over fetched ground, the map's sentence for missing ground gives
   way to "past the store's edge, N fetched tiles drawn outlined, outside line of sight",
   as §8.2's added check asks. While a tile is fetched, or after a fetch fails, the
   sentence says so. PN-09 counts "pinned P placed … fetched F placed from K one-degree
   tiles (n51e010), outside line of sight".
5. **Not a geographic store, not filled.** The worker reads the pinned store's coarsest
   tile once. If its own tags are not geographic, the fill source is refused on PN-09 and
   the map, by name.

§8.1 rows h to k are tested in `gungnir-app/tests/terrain_fill.rs` against a mirror on the
loopback interface. They stay drafts, outside the verification table. `ci.yml` runs them
with `terrain-fetch` in the test job's first shard.

## Amendment 7, 2026-10-31: the node reads the store, as built

§9's last sentence, "the node reads the same store for its line of sight", is built. Until
now `gungnir-node` read no terrain: its coverage answer was flat and said so. Four things
are worth stating:

1. **One path, moved down rather than copied.** The desktop's placement, its footprint and
   its field were in `gungnir-app` and `gungnir-viewport3d`, which the node may not depend
   on. They moved, unchanged, to crates both binaries already reach: `StoreFootprint` to
   `gungnir_data::terrain_store`; the agreement of baseline and manifest (§4.2, amendment 3
   item 4) and the coverage extent (§5.2) to `gungnir-config`, beside `local_frame`; the
   placement (GAP-023, GAP-108, GAP-197, GAP-200), the footprint's placement and the
   field's build to `gungnir_analytics::placement`, with a `crs` feature of its own that
   both binaries' `crs` turn on. `gungnir-analytics` has no edge to `gungnir-config`, so
   each binary reads its baseline's strings into `placement::Declared` itself. **No
   dependency edge is added.**
2. **The node's field is the desktop's.** Over one baseline the node's field equals the
   desktop's cell for cell, in the local frame and through PROJ in UTM
   (`gungnir-app/tests/terrain_tiles.rs`). It is built on a thread of its own, asked for
   again only when the coverage extent moves, and the thread is joined when the node stops
   (GAP-260). The coverage answer's line of sight reads it, and its parameters carry
   `terrain_masking_applied` only while it is held.
3. **The pinned store alone (D-164).** The node has no view and reads no fill tile. A
   baseline naming `terrain.fill_source` is said to be read by desktops for context only.
   One DEM named by `terrain.path` is still the desktop's alone, and the node says so.
4. **Said on the health line.** The node's health log carries a `terrain` field: no terrain,
   one DEM it does not read, a store refused and why, a field building, one not built and
   why, or the level and tiles it reads, with refused tiles and ground past the store's edge
   flat. A store in a projected frame is refused by name in a build without `crs`. A store
   in WGS84 degrees with ellipsoidal heights needs no PROJ and is read by any build. A
   height in a geoid datum is made ellipsoidal through the pinned grid, found where the
   desktop finds it and verified the first time a tile needs it.

## Traceability

GAP-227 (action, last item, widened); D-164; D-173; D-174; D-139; D-160 to D-163; GAP-252 and
`DN-35-terrain-in-the-three-d-scene.md`; DN-34 §13.1 row 8 and §13.2 step 6;
`../rust-3d-data-ecosystem-build-vs-adopt.md` §2.1 and §4; GAP-023, GAP-108, GAP-197, GAP-200;
GAP-247's height field; `ARCHITECTURE.md` §8; DN-12 §7;
`../review/2026-10-26-comprehensive-review.md` §8 finding 6.
