# GAP-252: terrain in the three-d scene, built

DN-35 (`../../design/DN-35-terrain-in-the-three-d-scene.md`) designed DN-34 §13.2 step 1: the
terrain drawn by GL under the layers, still straight down. The owner answered D-154 to D-158
the same day (amendment 1), and the DEM clip was committed with amendment 2. This item is the
build, the choices amendment 2 left to it, and what the owner checks on a display. The look
itself follows as its own record.

## What changed

- **One surface, built once per load.** `layers::terrain_surface` decimates the placed grid to
  the projection's 16 384-vertex budget (D-155), colours it by height over the full grid as
  before, lights it with the hillshade (D-157), and refuses by name a grid that does not fill
  `rows * columns` (`MalformedMesh`) or has no cell with four heights (`EmptyMesh`).
  `TerrainLayer` carries a `revision`, which `gungnir-app` sets on each placement
  (`TerrainStatus::Loaded::revision`, unique within the process). `ViewportState` holds the
  surface, or the refusal, keyed by that revision, so neither is recomputed per frame. The
  projection's `draw_terrain_2d` now projects the surface's positions and no longer folds the
  full grid every frame.
- **The hillshade** (amendment 1 item 1). Light from azimuth 315 degrees, 45 degrees up. The
  normal at each kept vertex comes from central differences between its kept neighbours, one
  sided where only one is kept, and flat on an axis with none. The factor is the light's cosine
  on the normal over its cosine on flat ground, so flat ground is exactly one and keeps today's
  colour. It is bounded to 0.75 to 1.25. Those bounds are what keep a dead spot over the lightest
  ground darker than the darkest ground with none, in both palettes. At 0.7 to 1.3 that test
  fails. The alpha stays `terrain_alpha`.
- **Two GL passes.** `Prepared3d` gains `ground_slot`, reserved after the metre grid and before
  `draw_layers`, beside GAP-251's `scene_slot`. `SceneRenderer::paint` is split into
  `paint_ground` and `paint_glyphs`. The ground pass holds one `Gm<Mesh, ColorMaterial>` per
  revision with `ColorMaterial::new_transparent`, and its colours cross to three-d
  unmultiplied. The status line moved after the glyph pass, so neither pass can cover it.
- **Who draws the ground.** `ground_source(wanted, resident)` decides once a frame: the scene
  draws it only when the ground pass already holds this revision, and egui draws it otherwise.
  The binary reads the resident revision under the lock it already takes, and a poisoned lock
  reads as none. On the frame a terrain arrives, egui draws the ground while the ground pass
  uploads it. The binary sets the ground callback whenever there is a surface to upload or a
  mesh to drop, so on that frame the slot holds a callback that draws nothing. DN-35 §8.1 row e's
  "slot left empty" is held as "everything in the slot is under the egui ground".
- **PN-09's terrain line** says `ground drawn by the scene` or `ground drawn by the overlay`,
  or `ground not drawn:` with the refusal.

No dependency edge, configuration field, route, event or schema changed. No `NotImplemented` was
added or removed.

## The headless rows (DN-35 §8.1, as amendment 1 restates them)

They are tests, and they are **drafts**: none is in `../../verification-capability-table.md`,
because a pass criterion is the owner's.

| Row | Test |
|---|---|
| a | `layers::tests::the_surface_is_the_projections_ground`, with today's `draw_terrain_2d` kept in the tests as `oracle_terrain_mesh`: a small grid, a grid past the budget and a grid with holes, each sloped and flat. Every vertex and index is the oracle's. Over flat ground every colour is too; elsewhere the alpha is the oracle's, and each component is the oracle's within the shade's bounds. The shade tests are `the_hillshade_is_one_on_flat_ground_and_bounded_on_any_slope` and `a_hole_changes_no_other_vertexs_shade`. The contrast is `a_dead_spot_stays_darker_than_any_shaded_ground`. |
| b | `layers::tests::the_surface_is_built_once_per_load` (a hundred frames, one build; a new revision, one; none drops it; a refusal held, not retried), and `gl::tests::the_ground_is_uploaded_once_per_revision` for the upload's key |
| c | `gl::tests::the_camera_puts_the_ground_where_the_projection_does`: up from -430 m to 8 850 m, every vertex within 0.05 point of the projection and inside the near and far planes, at the existing camera test's zooms, shapes and scales |
| d | `tests::the_ground_is_drawn_by_exactly_one_renderer`, every pairing, and `tests::the_hand_over_draws_the_ground_once_on_every_frame` through `prepare_3d`. `gungnir-app/tests/terrain.rs::pn_09_says_which_renderer_draws_the_ground` reads the hand-over on PN-09 |
| e | `tests::the_scene_paints_the_ground_under_the_layers_and_the_status_line_last`, stand-in callbacks in both slots |
| f | `gl::tests::a_terrain_colour_crosses_unmultiplied` |
| g | `gungnir-app/tests/frame_budgets.rs::the_scene_path_draws_no_egui_terrain_and_the_surface_is_built_once`. In every profile it asserts that the scene's egui pass draws no terrain mesh and the projection's draws one. The timings are printed and not asserted (figures below) |

FIGURES

## The Oregon baseline (amendment 2 item 2)

`testdata/usability/round-1-oregon.json`, written by `testdata/usability/tools/round_1_oregon.py`.
`../../../testdata/usability/SOURCE.md` has the full derivation. The choices:

- **The origin, 43.67 N, 123.25 W, at the ground's height under it**: 142.13 m ellipsoidal.
- **The radars at the ground under each plus round 1's own height as the mast**: S1 at
  142.13 + 25 m and S2 at 301.67 + 30 m. In round 1 those heights were above the sea.
- **The approach**, which the amendment left to the build. Round 1's axis runs about 24 km east,
  and its outer 5.4 km or so lie past the clip's east edge, where there is no ground to clear. So
  the whole axis is flown at one height, 840 m ellipsoidal: 300 m over the highest ground on the
  part of the axis the clip holds (539.6 m), as round 1's 300 m was over the sea. A per-point
  height was the alternative. It cannot be had for the outer point, and between points a
  straight segment could cut through a ridge.
- **The laydowns.** Each sensor placement is re-sited at the ground plus its mast. Resources
  keep round 1's placements, because the resources kept their offsets from the origin.
- **`vertical`: NAVD88 (`epsg:5703`)**: what USGS publishes 3DEP heights in, and what the tile's
  metadata states (`altdatum`: North American Vertical Datum of 1988, metres).
- **`nad83_realisation`: NAD83(2011) (`epsg:6318`)**, chosen against the tile's metadata,
  `USGS_1_n44w124.xml` (published 2025-08-05). Its `horizdn` is "North American Datum of 1983"
  with no realisation. It has no lineage naming the source DEMs, and its time period runs from
  1974 to 2022, so the file cannot settle it. NAD83(2011) is the realisation the USGS Lidar
  Base Specification requires of 3DEP lidar in the conterminous United States, and the one the
  newer, lidar-derived sources in that span would carry. A source in an earlier realisation
  differs by about a metre here, a thirtieth of a cell. For the look that is invisible, and the
  build is not claiming more than that. NAD83(2011) also needs no NADCON5 step, only PROJ's
  Helmert to ITRF2014.

The variant's own checks are in `gungnir-app/tests/terrain.rs`:
`the_oregon_variant_declares_what_the_clip_does_not_say` (every build), and
`the_oregon_clip_is_placed_under_round_1s_sector` (`crs` builds, run by `ci.yml`'s `proj-crs`
job). The second loads the variant through the desktop's own tick. The placed surface has to
pass through the origin within 10 m of up zero and lie about 30 m under S2. Both heights came
from the script's own decoder and `pyproj`, not from the loader. The grid has to be about 35 by
32 km, rising to the south-east, and the viewport has to build its surface from it.

## The default desktop cannot place this clip

**DN-35's look cannot be made on the Windows desktop as it is built today.** The clip is in NAD83
degrees, and converting a real-world CRS other than WGS 84 geographic needs the `crs` feature,
which links libproj. libproj cannot be built on Windows MSVC (D-51), and the desktop
`release.yml` publishes has no `crs`. That build refuses the clip by name. Even with the GEOID18
grid installed, PN-09 reads `... needs the crs feature, which this build was not compiled with`.
`a_build_without_crs_refuses_the_oregon_clip_by_name` holds that refusal, so it cannot become
a silent misplacement. Amendments 1 and 2 did not say where the look would run. The owner's
previous look ran on the Windows workstation.

Where the look runs is the owner's to decide. The options seen so far:

1. A Linux build with `--features crs` (as `ci.yml`'s `proj-crs` job builds it) on a machine
   with a display, or under WSLg. The look then exercises the real placement, as amendment 1
   item 2 intended.
2. A Windows build linked against a system libproj of at least 9.6.2, if one can be installed.
   This is untried.
3. A copy of the clip placed into round 1's local frame by a `crs` build, committed beside it
   for the Windows desktop. The look would then see the drawing only, and the placement would
   rest on the `crs` test alone.

## What the owner checks on a display

The session, from the repository root (the variant names the clip relative to it), on a build
that can place the clip (above). The pinned GEOID18 grid, `us_noaa_g2018u0.tif` (digest in
`../../../deploy/geoid/SHA256SUMS`), has to be on `PROJ_DATA` too. The variant names no
`geoid_grid_dir`, and without the grid the NAVD88 heights are refused by name. The committed
clip of it is for tests and does not carry the pinned digest.

```
GUNGNIR_CONFIG=testdata/usability/round-1-oregon.json
gungnir-app --rehearsal testdata/usability/round-1-seed.json
```

First, every check `the-owner-s-look-at-the-three-d.md` passed, and those of
`the-three-d-scene-draws-each-track-s.md`, over the new ground. Then DN-35 §8.2, with amendment
1 item 3 and amendment 2 item 3:

1. The scene's ground appears. Switched with the corner control, it is the projection's ground:
   the same colours, the same hillshade, the same translucency, the metre grid showing through.
2. *(Not checkable on this fixture.)* The no-data hole cannot be checked here: the clip has no
   no-data cell (amendment 2 item 3). Rows a and d hold it on their synthetic grids.
3. The dead spots, rings, gap line, geofence and boom lie on the ground in the same places in
   both renderers, and none is hidden under the ground.
4. Every disc is drawn over the ground, including where a track passes over high ground below
   it. The seed's tracks fly 120 to 300 m above the origin's ground, so in the south-east some
   pass beneath the surface (DN-35 §4 item 2). They are still drawn.
5. The status line and the hidden-layer sentence read clearly over the ground.
6. Panning and zooming moves the ground with the grid and the discs. Resizing the window,
   moving the panels, and display scaling at 100 % and 150 % lose nothing.
7. Nothing visibly flashes when the terrain finishes loading, and the hillshade appears with
   the terrain, not a frame after it.
8. PN-09's terrain line says `ground drawn by the scene` in the scene, and
   `ground drawn by the overlay` after switching to the projection.
9. The relief is the place's. The ridge lines and the high ground in the south-east fall where
   the USGS topographic map of the same area puts them, against the metre grid and the origin.
   This catches a swapped axis, a flipped row order or a wrong origin. It cannot see an error
   of a metre or a cell.

Until the owner has looked, the ground pass is unseen. The tests see the surface, the order of
the two passes and who draws the ground, but not what either GL pass draws.
