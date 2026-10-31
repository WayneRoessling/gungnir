# GAP-227: the staged terrain store, read and verified

The first increment of `docs/design/DN-36-staged-terrain-and-map-tiles.md`: §9's
`gungnir-config` and `gungnir-data` lines, and a tile placed through the single DEM's path.
Nothing is drawn from a store yet, and line of sight does not read one; those are §5's
increments, and the terrain line says so until they land.

**The baseline.** `TerrainConfig::path` is now optional, beside `store` and
`manifest_sha256`. Validation refuses both or neither, a store without a digest, a digest
that is not 64 hexadecimal digits, and a digest beside a path. All three are skipped when
absent, so a single-DEM baseline serializes as it did; that matters because a baseline's
signature covers its re-serialization (`ConfigBaseline::signing_bytes`), and a field
written as `null` would have broken every signed baseline with terrain in it.
`a_single_dem_baseline_serializes_as_before_the_store` holds the bytes.

**The store, `gungnir_data::terrain_store`.** `TerrainStore::open` reads `manifest.json`,
refuses it by name when its SHA-256 is not the baseline's, and checks its geometry
(`StoreIndex::from_manifest`): every tile's shape and bounds are derived from the extent,
the level and the tile size, and a manifest whose entries disagree is refused. `read_tile`
reads a tile once, hashes it, and decodes the same bytes, so nothing can change between
the check and the read; a tile whose digest differs is refused as "tile 1/1_0 refused:
digest: ...", and every other tile still reads. Files larger than a tile can be are refused
before they are hashed. A tile's path is built from three numbers, so no manifest can
point outside its store.

**Staging, moved forward.** `stage` cuts a grid into the pyramid, writes each tile with its
apron through the new `geotiff::write`, and writes the manifest last, returning its digest.
It refuses a directory that already holds a manifest. Each level carries the sum and count
of the level-0 cells under it, so a coarse cell is the mean of every source cell beneath it
and never a mean of means; `levels_average_the_cells_under_them` checks that against a
no-data cell. `terrain stage`'s command line and its fetch are still to come.

**The desktop.** A baseline naming a store sends `LoadRequest::TerrainStore` to the loader
thread. The manifest's frame must be the baseline's, and a vertical system or realisation
stated in both must agree; otherwise the store is refused by name and an alert raised.
A verified store becomes `TerrainStatus::StoreVerified`, held as `AppState::terrain_store`
for the next increment. Dead spots and the UAS ground model refuse it with the same line,
so neither claims ground it does not read. `place_tile` places a loaded tile through `place`;
`a_local_frame_tile_lands_where_the_single_dem_does`, and its UTM twin in a `crs` build,
place a tile and the whole source and find the same cells at the same positions.

**What the build changed in the design** is DN-36 amendment 3: the geometry made exact, no
TIFF predictor (`tiff` 0.11 cannot write one for floats), an empty tile left unstaged, and
the rule for the manifest's and the baseline's declarations.

**Not done.** §8.1's rows stay drafts outside the verification table. The node reads no
terrain today, single file or store; DN-36 §9 says it will read the store for line of sight,
and that waits on line of sight's increment.
