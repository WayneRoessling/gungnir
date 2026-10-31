# GAP-227: the node reads the pinned store for its line of sight

`docs/design/DN-36-staged-terrain-and-map-tiles.md` §9 said the node reads the same pinned
store for its line of sight. Until this change `gungnir-node` read no terrain at all, and its
coverage answer said line of sight was flat. DN-36 amendment 7 records the build.

**Why code moved instead of being written again.** Everything the desktop does to read a
store for line of sight lived in crates the node cannot reach: `place` and its helpers in
`gungnir-app/src/terrain.rs`, the footprint and the field's build in
`gungnir-app/src/terrain_tiles.rs`, and `StoreFootprint` in `gungnir-viewport3d`. A copy in
the node would have been a second placement path to keep equal by hand, and the coverage
answer a desktop and a node serve over one baseline would have drifted with it. GAP-194
judges a commander's accepted gap against the node's answer, so the two have to agree. So
the code moved, verbatim apart from its signatures:

- `StoreFootprint` to `gungnir_data::terrain_store` (pure geometry on `StoreIndex`), which
  `gungnir-viewport3d` re-exports.
- The agreement of baseline and manifest, `TerrainConfig::store_declaration`, and the
  coverage extent, `ConfigBaseline::coverage_extent`, to `gungnir-config`, beside
  `local_frame`, which moved there for the same reason under GAP-132.
- `place`, `place_footprint`, `place_store_tile` and `store_sight` to
  `gungnir_analytics::placement`, with a `crs` feature that both binaries' `crs` features
  turn on.

`gungnir-analytics` has a dev edge to `gungnir-config` only, and a runtime one is not drawn
in `ARCHITECTURE.md` §7.1. So the placement takes the baseline's declaration already read
into `gungnir-data`'s types (`placement::Declared`). Each binary reads its own strings into
it: the desktop through `terrain::read_declared` and its `geoid` helpers, the node through
its own `read_declared`. That costs one thirty-line mapping in the node, and no new edge.

**The node.** `gungnir_node::terrain::NodeTerrain` opens the store against
`terrain.manifest_sha256` at start, agrees it with the baseline, and hands it to a thread of
its own. The thread places the footprint, then paints a field for each coverage extent the
tick asks for. The tick asks again only when the extent moves. The thread is joined on drop,
since PROJ work running into the process's exit is the abort GAP-260 found. The geoid grids
a placement asks for are found by `gungnir_data::geoid::locate`, the desktop's rule, and
verified against their digests the first time a tile needs one. `coverage_answer` reads the
field through `HeightFieldLineOfSight`, and sets `terrain_masking_applied` only while it is
held. The health log gains a `terrain` field, logged once at start and on every health line.

**Tests.**
- `gungnir-node/tests/terrain_store.rs`: a local-frame store built and read over the
  extent, with ground past its edge flat and said; the field following the extent and
  dropped without one; a manifest refused by digest; a refused tile flat and counted; no
  terrain and a single DEM said; a fill source never read (D-164); a WGS84 geographic store
  placed by a build without PROJ; and a UTM store placed through PROJ under `crs`, refused by
  name without it.
- `the_coverage_answer_is_masked_by_the_pinned_store` in `gungnir-node/src/main.rs`: a
  ridge hides an approach that flat line of sight covers.
- `gungnir-app/tests/terrain_tiles.rs`: the node's field equals the desktop's bit for bit,
  in the local frame and, under `crs`, in UTM.
- The desktop's own terrain suites pass unchanged, with and without `crs`, against
  conda-forge PROJ 9.9 on Windows.

**CI.** `ci.yml`'s `crs` path filter gains `gungnir-analytics/`, which now holds
feature-gated code. The `proj-crs` job gains a step running both store suites under the
feature: the desktop's UTM store test in `terrain_tiles.rs` had not run anywhere before.

**Not done.** The node places no Category 129 height on the store (GAP-199 stays the
desktop's), and reads no single DEM. GAP-227 stays open for D-139's symbols, the cursor
readout and declutter (DN-37).
