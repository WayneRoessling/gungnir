# DN-35 Terrain in the three-d scene, with the layers on it

Closes GAP-252: rows 1 and 2 of `DN-34-sensor-dead-spots.md` §13.1, taken together as §13.2
step 1 orders them. Status: design, 2026-10-30; **nothing is built**. Four choices are
raised for the owner as D-154 to D-157 (§10), each with a recommended default. The build
waits for the owner's answers. What the owner has signed of this note is in
[`../signatures.md`](../signatures.md).

A note of its own, not a DN-34 amendment. DN-34's amendments record what was built from
DN-34's own design; §13 is an inventory of later work, and each row in it needs its own
design. DN-34 amendment 6 points here.

## 1. The increment, and what it is for

§13.2 step 1 reads: "Terrain in GL with the overlay layers moved onto it (rows 1 and 2
together), still straight down, so what the scene shows is what the projection shows,
draped."

**On a straight-down orthographic view, nothing the operator sees should change.** That
is not a weakness of the increment. Since amendment 5 the camera is orthographic and sized
in egui points, so a point's place on screen does not depend on its height. A surface in GL
and the same surface drawn by egui put the same colour on the same pixels, and a layer
draped on the ground lands where the flat overlay already draws it. What the increment buys
is underneath:

1. **The surface exists in GL**, so the tilt (§13.1 row 3) has a ground to tilt over and
   does not have to build one as well.
2. **The scene's egui pass stops building the terrain every frame.** The GL pass draws a
   mesh uploaded once, and egui draws only the layers.
3. **The surface is computed once per load for both renderers.** Today `draw_terrain_2d`
   reads every vertex of the full grid every frame (§2) to find the colour ramp's lowest and
   highest heights. A 1 000 by 1 000 DEM is a million reads a frame before any drawing.

So the owner's look at this increment checks for one result: nothing changed. That
criterion is clear, and it is the right one for a second change to a path no test can see.

## 2. What exists

Read from the code at `f9778a32`:

| What | Where | What it gives this note |
|---|---|---|
| The 2D terrain | `gungnir-viewport3d/src/layers.rs::draw_terrain_2d` | Builds an `egui::Mesh` **every frame**: folds every vertex of the full grid for the lowest and highest height, keeps every `terrain_stride`-th row and column so at most `TERRAIN_VERTEX_BUDGET` (16 384) vertices remain, projects each, colours it by `terrain_color` at `palette.terrain_alpha`, and leaves a hole wherever a corner has no height. No hillshade: the "shading" is the height ramp |
| The terrain input | `layers::TerrainLayer` (positions, rows, columns), from `gungnir-app/src/terrain.rs::layer` | Borrowed from the loaded `TerrainMesh`. **Carries no revision.** The terrain loads once per session (`terrain::start` is idempotent), so nothing has needed one |
| The GL scene | `gungnir-viewport3d/src/gl.rs::SceneRenderer` | One instanced sphere mesh and the palette, both created in `attach`. `paint` uploads the instances and calls `render_partially` into egui's clip box. Clears nothing |
| The order on screen | `lib.rs::prepare_3d`, then `gungnir-app/src/main.rs` | `prepare_3d` paints the background, the metre grid, **every layer** through `draw_layers`, the bearing rays and the status line. The binary then adds the paint callback. **The GL draw lands on top of everything egui painted in the viewport before it.** Harmless while it draws only discs. PR #269 (GAP-251, open as this is written) keeps that order up to the status line, then reserves a slot for the callback (`Prepared3d::scene_slot`, an `egui::Shape::Noop` the binary fills with `Painter::set`) and draws each track's frame, labels and assignment after it. A ground in that one callback would still cover every layer |
| Depth | `gungnir-app/src/main.rs`'s `NativeOptions` | Sets only the renderer. eframe 0.34.3's default `depth_buffer` is **0** (`epi.rs`: "egui doesn't need the depth buffer"). **The window has no depth buffer**, so draw order is the only depth there is. three-d's materials enable a depth test, and with no depth buffer OpenGL passes every fragment |
| The camera | `gl::camera` | Orthographic, straight down, 100 km above the origin's up, far plane at 200 km. `the_camera_puts_a_track_where_the_projection_does` holds a point from −40 m to 9 000 m up to `TopDownView::project` |
| The one-`draw_layers` rule | `lib.rs::draw_layers` | One function draws terrain, point clouds, dead spots, geofences, rings, gaps, hazards, predictions, the dense group, the laydown preview and the hidden-layer sentence, for both renderers. Terrain, point clouds, predictions, the dense group and the preview have no toggle |
| The fixtures | `testdata/dem/`; `gungnir-app/tests/dead_spots.rs::ridge_dem` | The committed DEMs are a few cells each. The ridge DEM the dead-spot test uses is written by the test into a scratch directory. Round 1's baseline names no terrain |

## 3. The trap

**A GL terrain added to the existing callback would be drawn over every layer and over the
status line.** The dead spots, rings, gaps, geofences and hazards would sit under a
translucent wash, or vanish under an opaque one. The status line would be covered too, and
it is what `ARCHITECTURE.md` §10 items 52 and 53 rely on in place of a test: an empty scene
that says twelve tracks is a fault someone can report. Nothing headless would see any of
this. `RenderProbe` reads egui's shapes, and they would all still be in the shape list,
painted and then painted over.

So this note is about draw order first and geometry second.

## 4. The depth question (§13.1 row 1)

Row 1 named the risk as "a terrain mesh under flat overlays makes the egui overlay ...
float at ground level zero while the ground rises". It has four parts.

1. **On screen there is no float.** With an orthographic camera looking along the frame's
   up, a point's screen position does not depend on its up. A ring drawn at up zero and the
   same ring draped at the ground's up cover the same pixels, and the existing camera test
   already shows this for a point anywhere from −40 m to 9 000 m. The float becomes real at
   the tilt, and §13.1 row 3 owns it.
2. **Draw order is the depth, and the order is the projection's.** Background and grid
   (egui), then the ground (GL), then the layers (egui, one `draw_layers`), then the bearing
   rays (egui), then the glyphs (GL), then each track's frame, labels and assignment (egui,
   PR #269), then the status line and the corner control (egui). That is the projection's own order, with two of its steps drawn by GL.
   "A track symbol is never under anything" (GAP-007) holds because the glyph pass comes
   after every layer, not because of a depth test. A track flying lower than the ground's
   up at its position, which a sensor's height error can produce over a ridge, is still
   drawn.
3. **The ground lies inside the clip volume.** Near is at the camera, far at 200 km, and
   the camera is 100 km above up zero, so the volume holds every up from −100 km to
   +100 km. Every DEM on Earth fits, with the curvature drop a placed grid carries at
   100 km (about 785 m) included. A test holds this with the fixture's extremes (§8 row c).
4. **No depth buffer is requested in this increment.** Straight down, a placed grid never
   covers itself, so nothing needs one. The tilt does need one: a ridge hides the valley
   behind it. Asking for one changes the whole window's framebuffer, so that is row 3's to
   decide, with its cost stated there.

## 5. Two GL passes, and still one `draw_layers`

### 5.1 The passes

`SceneRenderer::paint` splits into two:

- `paint_ground(surface, view, viewport, clip, pixels_per_point)` draws the terrain mesh, if
  it holds one for the revision the frame wants (§7). The terrain is translucent, as in the
  projection, so the background and the metre grid painted before it show through as they
  do in 2D. The material is `ColorMaterial::new_transparent`. A straight-down grid never
  overlaps itself, so no sorting is needed.
- `paint_glyphs(...)` is today's `paint`, unchanged.

Each pass is its own `egui::PaintCallback`. The binary builds them, because the callback
type belongs to `egui_glow` and only the binary depends on eframe. The viewport crate decides
where they go, by the slot PR #269 introduces: `prepare_3d` reserves a place in the paint
order with an `egui::Shape::Noop`, and the binary fills it with `Painter::set`. This note
adds a second slot. `Prepared3d` gains `ground_slot`, reserved after the grid and before
`draw_layers`, beside #269's `scene_slot` for the glyph pass. **The order is one function in
`gungnir-viewport3d`, and a test can read it** (§8 row e), as #269's tests already read
what is painted after the scene's slot. The status line and the corner control move after
the glyph pass, which also stops the discs drawing over the status line as they can today.

### 5.2 The rule

`draw_layers` gains one argument, which renderer draws the ground this frame:

```rust
/// Who draws the terrain this frame (DN-35 §7). Decided once a frame, before anything is
/// drawn, and handed to both `draw_layers` and the ground pass, so the ground is drawn
/// exactly once: never twice, which would double its alpha, and never by neither while a
/// surface is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    /// egui draws it in `draw_layers`: the projection always, and the scene until the GL
    /// pass holds this revision's surface.
    Overlay,
    /// The scene's ground pass draws it; `draw_layers` skips `draw_terrain_2d`.
    Scene,
}
```

Every toggle is still read in the same function, and every layer that can be hidden is drawn
by the same call in both renderers. The terrain is the one thing that changes renderer, and
it has no toggle. That is the rule DN-34 §13.3 states, "one `draw_layers`, one `LayerInputs`,
one set of toggles for both renderers", and it holds unchanged. Point clouds stay in egui
(§13.1 row 7 is their GL increment).

### 5.3 What this does to row 2, the drape

With the layers in egui between the two passes, the scene shows each layer on the ground
exactly where a GL drape would put it, pixel for pixel, because the camera is orthographic
and straight down (§4 item 1). Row 2 as written builds the drape in GL: the dead-spot image
as a `three_d::Texture2D` on the mesh's texture coordinates, and the rings, gaps, geofences
and hazards as line geometry lying on the surface. Built now, that is a second
implementation of five layers on the one path no test can see. It would draw the same pixels
the egui layers already draw, until the tilt makes them differ. **D-154** asks whether the
drape waits for the tilt.

## 6. The surface, built once per load

A pure function in `layers.rs` builds what both renderers draw:

```rust
/// The placed terrain as both renderers draw it: the decimated grid, its colours and its
/// triangles, built once per load (`rust-ui-architecture-coding-standards.md` §5), never
/// per frame.
pub struct TerrainSurface {
    /// The `TerrainLayer::revision` this was built from.
    pub revision: u64,
    /// ENU metres, the kept vertices only; a no-data corner keeps no vertex.
    pub positions: Vec<[f32; 3]>,
    /// `terrain_color` per vertex, over the **full** grid's lowest and highest height, as
    /// `draw_terrain_2d` colours it today.
    pub colors: Vec<egui::Color32>,
    /// Two triangles per decimated cell whose four corners all have a height.
    pub indices: Vec<u32>,
}

pub fn terrain_surface(
    palette: &theme::Palette,
    layer: TerrainLayer<'_>,
) -> Result<TerrainSurface, ViewportError>;
```

- **`TerrainLayer` gains `revision: u64`.** `gungnir-app` sets it when it places a terrain
  and changes it on any later placement. Today that means once a session.
- **`ViewportState` holds it as `Option<Arc<TerrainSurface>>`**, rebuilt when the layer's
  revision differs from the one held, and dropped when the layer is `None`. This follows
  the pattern `sync_dead_spot_texture` already uses. The per-frame callback clones the
  `Arc`, a pointer, and never the surface.
- **The projection reads it too.** `draw_terrain_2d` projects the surface's positions into
  an `egui::Mesh` each frame and stops folding and decimating the grid each frame. Its
  output does not change. §8 row a holds the new path to the old function, which is kept in
  the tests as the oracle.
- **The GL mesh is built from it once.** `SceneRenderer` holds
  `Option<(u64, Gm<Mesh, ColorMaterial>)>`. The ground pass builds it inside the GL context
  the first time it is handed a revision it does not hold, and drops it when handed none.
  three-d 0.19's `Mesh::new` returns no error, so every refusal happens in
  `terrain_surface`, before the upload: a grid with no triangle is `EmptyMesh`, and
  positions that do not fill `rows * columns` are `MalformedMesh`. Both variants exist
  already.
- **Colours cross unmultiplied.** `egui::Color32` stores premultiplied components. The glyph
  path reads `.r()` and gets away with it because track colours are opaque. The terrain is
  translucent, so it must cross with `to_srgba_unmultiplied`, or the scene's ground comes
  out darker than the projection's by the alpha. §8 row f tests it.
- **What the build costs, once.** One pass over every vertex for the ramp, and one over the
  kept vertices for the mesh. That is estimated at a millisecond or two over a 1 000 by
  1 000 DEM, on the tick that sees the load. The build measures it in
  `gungnir-app/tests/frame_budgets.rs`. If one tick's cost threatens the egui budget, the
  build moves to the terrain worker, as amendment 1 item 7 moved the dead-spot fold.
- **Resolution.** The surface keeps the projection's decimation, so both renderers draw the
  same triangles. **D-155** asks whether the scene should take more.

## 7. The 2D shaded terrain stays the fallback

1. **The projection is the fallback renderer and is unchanged in what it draws.** Same
   triangles, same colours, same holes. Only the place its surface is computed moves.
2. **In the scene, the ground falls back per frame.** A pure function decides it:

   ```rust
   /// The scene draws its own ground only when the GL pass already holds the surface for
   /// the revision this frame wants; otherwise egui draws it, as the projection does.
   pub fn ground_source(wanted: Option<u64>, resident: Option<u64>) -> Ground;
   ```

   `resident` is read from the renderer under the mutex the binary already takes. A
   poisoned lock reads as `None`, so a renderer that panicked last frame leaves the 2D
   ground under the layers. The decision is made once, before anything is drawn, and both
   `draw_layers` and the ground pass are handed it. On the first frame after a load, egui
   draws the ground while the ground pass uploads the mesh and draws nothing. From the next
   frame, the scene draws it. No frame draws the ground twice, and no frame with a surface
   draws it never.
3. **With no terrain, neither draws any**, as today, and PN-09's terrain line says why. Its
   `Loaded` line gains which renderer draws the ground in the scene: `ground drawn by the
   scene` or `ground drawn by the overlay`. That is the one place the hand-over can be read.
4. **The corner control still switches to the projection at runtime**, as the record of the
   owner's look requires.

## 8. What can be tested headlessly, and what only the owner can see

### 8.1 Headless

Drafted as rows for `../verification-capability-table.md` §2. A pass criterion is
human-owned. These are drafts until the owner agrees them, and are not gates.

| Row | Test | Criterion |
|---|---|---|
| a. The surface is the projection's | `gungnir-viewport3d` unit tests | Over a small grid, a grid past the budget (stride above one), and a grid with no-data holes: the egui mesh built from `terrain_surface` equals the one today's `draw_terrain_2d` builds, kept as the oracle, vertex for vertex, colour for colour and index for index |
| b. Built once | the same | Over a hundred frames at one revision the surface is built once. A new revision rebuilds it once. `None` drops it. The GL upload's key, `(revision held, revision wanted)`, is the same pure test |
| c. The scene puts the ground where the projection does | `gl.rs` tests, beside `the_camera_puts_a_track_where_the_projection_does` | Every surface vertex of a fixture whose up runs from −430 m to 8 850 m lands within 0.05 point of `TopDownView::project`, and inside the camera's near and far planes, at the three zooms, viewport shapes and display scales the existing test uses |
| d. The ground is drawn exactly once | `ground_source` unit test | For every pairing of wanted and resident revision, `None` included, `Scene` only when both are `Some` and equal. A frame driven through the hand-over draws the 2D ground on the load frame and the GL ground after |
| e. The order on screen | `gungnir-viewport3d/src/lib.rs` tests, walking egui's output shapes with stand-in callbacks in both slots, as PR #269's `painted_over_the_scene` does for one | In the scene: background, grid, ground callback, dead-spot image, the layers in `draw_layers`' order, the bearing rays, the glyph callback, each track's frame and labels, the status line. With `Ground::Overlay`: the terrain mesh before the dead-spot image and the ground slot left empty |
| f. Colours cross unmultiplied | `gl.rs` unit test | A `terrain_color` at `terrain_alpha` reaches three-d's `Srgba` with the unmultiplied components and that alpha |
| g. The scene's egui pass with terrain | `gungnir-app/tests/frame_budgets.rs`, release | Over the 1 000 by 1 000 synthetic DEM in the scene path: the egui pass p99 within the existing 8 ms budget and no terrain mesh in its shape list. The once-per-load build is measured and stated |

Rows a, b, d and e also protect the projection, the path the tests can see, from the
refactor in §6.

### 8.2 Only on a display

The GL draw is still invisible to every test. The owner looks again, at the fixture of §9,
with these checks. They become the record of the look, as
`../record/2026-10-30/the-owner-s-look-at-the-three-d.md` was for increment 0. Every check
amendment 5's look passed is repeated, and these are added:

1. The scene's ground appears, and switched with the corner control it is the projection's
   ground: the same colours, the same translucency, the metre grid showing through.
2. The no-data patch is a hole in the same place in both.
3. The dead spots, rings, gap line, geofence and boom lie on the ground in the same places
   in both, and none is hidden under the ground.
4. Every disc is drawn over the ground, including where a track passes over the ridge.
5. The status line and the hidden-layer sentence read clearly over the ground.
6. Panning and zooming moves the ground with the grid and the discs. Resizing the window,
   moving the panels, and display scaling at 100 % and 150 % lose nothing.
7. Nothing visibly flashes when the terrain finishes loading (the one hand-over frame of §7
   item 2).
8. PN-09's terrain line says the scene draws the ground.

## 9. What the owner looks at (D-156)

Round 1's baseline names no terrain, and the committed DEMs are a few cells each. The look
needs a ground worth looking at: a ridge the round-1 approach crosses, so a dead spot shows
and a track passes over high ground, and a no-data patch. The recommended fixture is
synthetic: an ESRI ASCII grid in the `local-enu` frame, so no CRS or vertical datum is
involved, committed under `testdata/dem/`, beside a variant of the round-1 baseline that
names it. It is deterministic and small, and has no licence to carry. **D-156** asks whether
the look should be over real terrain instead.

## 10. The decisions

| Id | Choice | Recommended default | The alternatives |
|---|---|---|---|
| **D-154** | Where the layers lie on the scene's ground | **egui, between the two GL passes** (§5). Exact on the orthographic straight-down view, one implementation per layer, and the drape in GL is built with the tilt, the first increment where it would draw a different pixel | The drape in GL now, as §13.1 row 2 is written: the dead-spot texture on the mesh and the vector layers as line geometry on the surface, the egui layers withheld in the scene |
| **D-155** | The scene's surface resolution | **The projection's decimation**, at most 16 384 vertices, so both renderers draw the same triangles and §8 row a can hold them to each other | A finer budget for the scene alone (for example 262 144 vertices, 512 by 512), which a GPU draws easily but which shows relief the projection smooths away, so the two renderers would no longer show the same ground |
| **D-156** | The ground the owner looks at | **A committed synthetic DEM over round 1's sector** (§9) | A clip of a real DEM (for example Copernicus GLO-30) over a real place, with its licence and attribution added to the repository's notices |
| **D-157** | Whether the ground gains a hillshade in this increment | **No.** The height ramp stays as it is, so the look's criterion stays "nothing changed". A hillshade, once the GL ground has been seen, is a small follow-on to both renderers through `terrain_surface`'s colours | A hillshade now, in both renderers, computed once per load into the vertex colours. It makes a ridge readable straight down, which is what a dead spot is read against, and it changes the verified projection in the same increment |

## 11. Interface delta

- `layers::TerrainLayer` gains `revision: u64`. `layers::TerrainSurface`,
  `layers::terrain_surface`, `Ground` and `ground_source` are new.
- `draw_layers` takes a `Ground`. `Prepared3d` gains `ground_slot` beside PR #269's
  `scene_slot`, and the binary sets the ground callback into it.
- `gl::SceneRenderer::paint` becomes `paint_ground` and `paint_glyphs`, and the renderer
  reports the surface revision it holds.
- `ViewportState` holds the `Arc<TerrainSurface>`.
- PN-09's terrain line says which renderer draws the ground in the scene.

**No dependency edge**: `gungnir-viewport3d` already has `three-d` and `egui`, and
`gungnir-app` already builds `TerrainLayer`. **No configuration, no route, no event, no
schema change**: `SCHEMA_VERSION` stands. No `NotImplemented` is added or removed.

## 12. What this note does not do

- **No tilt, and no depth buffer.** §13.1 row 3, and §4 item 4.
- **No drape in GL**, under D-154's default.
- **No hillshade, no finer surface**, under D-157's and D-155's defaults.
- **No change to the glyphs, the bearing rays, or the track details.** PR #269 (GAP-251)
  draws each track's frame, labels and assignment over the scene's discs, as egui shapes
  after the glyph slot. This note builds on that slot and keeps those shapes where #269
  puts them. The build of this note starts from main with #269 merged.
- **No point clouds in GL** (§13.1 row 7) and **no basemap** (row 8).

## Traceability

GAP-252; D-154, D-155, D-156, D-157; DN-34 §13.1 rows 1 and 2, §13.2 step 1, §13.3,
amendment 5; GAP-226, D-153; GAP-251 (PR #269); GAP-007, GAP-022, GAP-023; CAP-5.9, CAP-5.10; PN-02, PN-09;
`ARCHITECTURE.md` §9 and §10 items 52 and 53; `../rust-ui-architecture-coding-standards.md`
§5; `../record/2026-10-30/the-owner-s-look-at-the-three-d.md`;
`../performance-budgets.md` (the egui pass).
