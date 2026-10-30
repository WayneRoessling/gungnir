# DN-34 Sensor dead spots: terrain shadow as a field, drawn as a layer

Closes GAP-246, and GAP-247 on the way. Status: first draft, 2026-10-29; **built the same
day**, with what the build changed in amendment 1 at the end. PN-16's per-option column
(§7 item 5) was built after, as GAP-248 (amendment 3). Raised outside the plan-11 set,
from the owner's request of 2026-10-29: show
the places where the terrain blinds the sensors to an approaching UAS or aircraft, as a
darkened area on the map an operator reads at a glance, and say what other three-dimensional
elements should follow it. What the owner has signed of this note is in
[`../signatures.md`](../signatures.md).

Three decisions were raised for the owner, D-146 to D-148 (§4, §9 and §12), each with a
recommended default; the owner took all three to the default on 2026-10-29
(`../mission/gap-analysis/data/decisions.yaml`). §13 is the analysis of the other 3D
elements, in the order the existing code lets them be built.

## 1. The gap and the thread step it blocks

Every sensor on the picture is drawn as a ring, or a wedge for a sectored sensor, out to
its nominal range (PN-11, `gungnir_viewport3d::layers::CoverageCircle`). The ring is the
sensor's claim. Terrain masking, which is what turns a claim into what the sensor sees, is
applied in exactly one place: along the approach polylines the baseline declares,
`combined_coverage` samples each one and tests line of sight at each sample (DN-12 §5).
Everywhere else on the map the ring stands unqualified. A valley behind a ridge that no
approach was drawn down is painted as watched, and a sensor manager reading the map
trusts the ring; `docs/mission/operational-environment.md` §2 says low-flying drones and
cruise missiles route along exactly those valleys because the radar horizon and the
terrain, not the radar's power, set the detection range against them.

The thread steps are MT-09 step 2, "compute coverage and gaps over terrain", and MT-07
step 3, restoring coverage after a loss; the sensor manager's task 1.2 and the planner's
2.2 are both "coverage over terrain" (`../ux/task-analysis/sensor-manager.md`,
`planner.md`), and the commander persona names being asked to accept a gap without seeing
it on the map as a frustration (`../review/2026-10-26-comprehensive-review.md` §8 finding
3). The approach gaps answer those steps along lines. A dead spot is an area, and the
system computes no areas.

**A dead spot, for this note:** a place in the air above the terrain that at least one
searching or tracking sensor claims, by range, sector and elevation band, and that no such
sensor can see, because the ground between is in the way. It is a claim the map makes and
the terrain denies, which is the one kind of coverage error nobody can read off the
picture today.

## 2. What exists, and what is missing

### 2.1 The inventory

| What | Where | State, and what it gives this note |
|---|---|---|
| A placed terrain surface | `gungnir-data::geospatial::TerrainMesh`, placed by `gungnir-app/src/terrain.rs` (GAP-023, D-41, GAP-108, GAP-197, GAP-200) | **Built.** ESRI ASCII and GeoTIFF DEMs, converted from a real-world CRS and a stated vertical datum to local ENU with each vertex's up a real height; a no-data cell is a hole (`NaN`), never a wall. Loaded off the render thread; `TerrainStatus` says loading, loaded, failed or not configured, and PN-09 draws the line |
| A ground height under a position | `gungnir-app/src/ground.rs::PlacedGround::up_at` (GAP-199, D-130) | **Built, for one consumer.** Interpolates the mesh's up across the triangle under an east and north, with a coarse-grid search because a placed grid is bent by its projection. Built to place a Category 129 height above ground; it lives in `gungnir-app`, where the analytics cannot reach it |
| Line of sight | `gungnir_analytics::{LineOfSight, FlatTerrainLineOfSight, TerrainMaskLineOfSight}` | **Built, as a reference.** `TerrainMaskLineOfSight` samples the segment every `sample_spacing_m` and takes the **nearest vertex's** height at each sample, which its own comment calls "correct, O(vertices) per sample, and the baseline any spatially indexed version must agree with". No indexed version exists |
| A viewshed over a point list | `gungnir_analytics::viewshed` | **Built; no caller** outside the crate's tests |
| A sensor's volume | `gungnir_analytics::CoverageVolume` (DN-12 amendments 1 and 2) | **Built and gated.** Range, azimuth sector in the frame, elevation band against the sensor's own vertical; `covers(p)` decides a point. `coverage_from_registry` admits only searching and tracking sensors; `volume_in_frame` is the one place a volume is built from a declaration, so PN-11, PN-16, DN-13 and the node place a sensor identically |
| Coverage along approaches | `gungnir_analytics::combined_coverage`, `CoverageReport` with its `CoverageParameters` | **Built, wired, gated** (GAP-006, GAP-118, GAP-158). One-dimensional: samples along declared polylines, grouped into gaps of two severities. The report carries its spacing and whether terrain masking was applied, so a flat answer cannot pass as a masked one |
| Where it is computed | `gungnir-app/src/sustainment.rs::coverage_report`, masked when a DEM is loaded; `gungnir-node/src/main.rs`, always flat (the node loads no terrain, D-130) | **Built.** Computed on the desktop tick, several times a frame: PN-02's layer, PN-11, PN-01's strip and the acceptance ledger each call it |
| Gap acceptance | DN-33, `gungnir-app/src/gap_acceptance.rs` | **Built.** An acceptance names a gap by approach, severity, extent, spacing and the terrain-masking flag, and re-opens on any shape change (DN-33 §5) |
| The viewport | `gungnir-viewport3d`: `TopDownView`, `layers.rs`, `gl.rs` | **Built.** Every layer, terrain included, is drawn by egui through one `draw_layers`, under the glyphs, in both renderers (GAP-007). The terrain is a shaded, translucent, stride-decimated mesh (`TERRAIN_VERTEX_BUDGET`). The three-d scene draws **one instanced sphere per track and nothing else**; its camera is derived from the 2D view's centre and zoom, looking straight down, so the two renderers frame the same ground. The scene has never been seen on a display (GAP-226), the picture has no basemap or standard symbology (GAP-227, D-139), and 3D Tiles streaming is three `NotImplemented` stubs (`../unbuilt.md`) |
| The budgets | `../performance-budgets.md`; `../mission/measures.md` MOP-14 | Coverage recompute over terrain under **10 s** after a sensor change; the egui pass p99 under **8 ms**; 60 fps with the viewport open |
| A radar horizon | `gungnir-sensor-sim::SensorParams::horizon` (DN-32) | **Built, for rehearsal only.** A 4/3-earth horizon over a flat earth; no terrain. Nothing in the live picture models a horizon at all |

### 2.2 What is missing

1. **No area.** Coverage is measured along lines somebody declared. Between the lines the
   map claims coverage by ring alone, and the ring is nominal range with no terrain in
   it. The capability the owner asked for is a field: for every place in the sector, which
   sensors see it.
2. **The terrain line of sight cannot be run over an area, and is already too slow on
   the lines.** `TerrainMaskLineOfSight::height_at` scans every vertex of the mesh for
   every sample of every ray. With a realistic DEM, 1 000 by 1 000 cells at 30 m, that is
   a million distance computations per sample. A 20 km ray at the default 250 m spacing
   has 80 samples; two 40 km approaches have 320 samples; three active sensors give 77
   **thousand million** vertex visits per report, and the desktop computes the report
   several times a frame (§2.1). The tests pass because `gungnir-app/tests/terrain.rs`
   and the analytics tests use grids of a few hundred vertices. This is **GAP-247**: a
   latent stall that a deployment's first real DEM would trip, filed on this note's
   reading of the code and **not measured**. A field needs a height lookup that costs
   nothing per vertex, and the approach gaps need it first.
3. **No stated height.** A terrain shadow is three-dimensional: a UAS at 50 m above the
   ground is hidden behind a ridge that an aircraft at 3 000 m is not. A darkened area
   that does not say what height it was computed for claims more than it knows, which is
   the failure DN-12 §5 rule 4 exists to prevent for spacing.
4. **Nothing draws an area with a meaning.** The layers are rings, polylines, markers and
   a shaded surface. There is no raster layer, no palette token for a shadow, and no
   sentence for a layer that could not be computed because there is no terrain.
5. **Nothing computes off the tick but the loaders.** The approach report is cheap enough
   to recompute every frame only because the grids are small (item 2). A field over a
   sector is not, and MOP-14's 10 s budget is a budget for a recompute, not for a frame.

## 3. The owning components

| Concern | Crate |
|---|---|
| The regular height field, its resampling from the placed mesh, the bilinear lookup and the indexed line of sight | `gungnir-analytics` (DN-12's crate; it already depends on `gungnir-data`, which owns `TerrainMesh`) |
| The shadow field, its measure and its parameters | `gungnir-analytics` |
| Computing it off the render thread, when, and holding the result and its age | `gungnir-app` (a worker beside the terrain loader, the pattern `terrain.rs` and `pointcloud.rs` already use) |
| The layer, its texture, its sentence and its toggle | `gungnir-viewport3d` (`layers.rs`), `gungnir-ui` (PN-11), `theme.rs` for the tokens |
| The heights and the cell size | `gungnir-config` (`AnalyticsConfig`) |

**This note adds no dependency edge.** `gungnir-analytics` reaches `gungnir-data` and
`gungnir-model`; `gungnir-app` reaches both and `gungnir-viewport3d`; the viewport takes a
plain struct the app has already built, as it takes `TerrainLayer` and `GapPolyline`.
`PlacedGround`'s interpolation moves down into `gungnir-analytics` as the height field's
lookup, and `ground.rs` reads it from there, which removes a duplicate rather than adding
an edge.

## 4. What the shadow means (D-146)

A field is a grid of cells over the sector, each cell a column of air at **one stated
height above the ground**. For each cell the field records how many active sensors claim
it and how many of those see it:

| Cell | Meaning | Drawn |
|---|---|---|
| **No data** | The DEM has no height here, or the cell lies off the DEM | Yes, as unknown: a neutral hatch. Unknown is not clear |
| **Unclaimed** | No active sensor's volume contains the probe | No. The rings already say it, and this layer is about what the rings get wrong |
| **Shadow** | Claimed by at least one active sensor; seen by none | Yes: the dark fill. **This is the dead spot** |
| **Single** | Seen by exactly one | Yes: a hatch in the gap colour, DN-12's single-sensor severity made an area |
| **Fused** | Seen by two or more | No |

Rules:

1. **Only sensors that are searching or tracking count**, as DN-12 §5 rule 1 and
   `coverage_from_registry` already hold. A standby sensor claims nothing, so its ring is
   not drawn and its shadow is not either.
2. **The field is computed at a stated height above ground**, and the layer says which.
   Several heights may be configured (§9); one is shown at a time, chosen on PN-11.
3. **Shadow is a claim the terrain denies**, not every place nobody watches. The owner's
   request was the terrain's blind spots, and the distinction is operationally real: a
   place beyond every ring is read correctly from the rings, while a place inside a ring
   and behind a ridge is read wrongly from them. Drawing both the same would turn the
   layer into a second, darker copy of the rings' complement and hide the one thing it
   exists to show.
4. **No terrain, no field.** With no DEM configured, loading, or refused, the layer is
   not computed and the viewport says so in the muted colour, the same discipline as
   `NoCoverage::NoOrigin`: the dead spots are not absent, they are unknown. A flat-earth
   shadow field is never drawn, because on a flat earth there is none, and an empty
   shadow layer over a sector with a ridge in it would be the layer lying.
5. **A single-sensor cell is drawn**, because one sensor gives a bearing and a range and
   not a fusible track (DN-12 §5 rule 2), and the same stretch of air that is watched by
   one radar fails on the first loss.

**D-146** asked the owner to confirm rule 3, the one choice in this table that changes the
operator's reading: dark means *claimed and hidden by terrain*. The alternative, dark
means *seen by nobody for any reason*, is simpler to state and was recommended against for
the reason in the rule. **Taken by the owner, 2026-10-29: rule 3 stands as written.**

## 5. Types

In `gungnir-analytics`:

```rust
/// The placed terrain resampled onto a regular ENU raster: one up per cell, NaN where the
/// mesh has no height, so a lookup costs nothing per vertex.
pub struct HeightField {
    /// ENU east and north of the south-west corner, metres.
    pub origin_enu: [f64; 2],
    pub cell_m: f64,
    pub rows: u32,
    pub columns: u32,
    /// `rows * columns` ups, metres, north row first; NaN is no data.
    pub up_m: Vec<f32>,
}

impl HeightField {
    /// Resample a placed mesh at `cell_m`, by the same triangle interpolation
    /// `PlacedGround::up_at` does today, once per load. Refuses a mesh that is not a
    /// grid, or a cell size that is not finite and positive.
    pub fn resample(mesh: &TerrainMesh, cell_m: f64) -> Result<Self, HeightFieldError>;
    /// Bilinear up at an east and north; `None` off the field or touching a no-data cell.
    pub fn up_at(&self, e: f64, n: f64) -> Option<f64>;
}

/// Line of sight against a height field: the segment sampled at the field's own cell
/// size, each sample against the bilinear surface. O(length / cell) per ray.
pub struct HeightFieldLineOfSight<'a> { pub field: &'a HeightField }
impl LineOfSight for HeightFieldLineOfSight<'_> { /* ... */ }

/// What a shadow field was computed with, carried on the result (DN-12 §5 rule 4).
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShadowParameters {
    pub height_agl_m: f64,
    pub cell_m: f64,
}

/// One cell of the field (§4).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CellSight {
    NoData,
    Unclaimed,
    /// Claimed by `claimed` sensors, seen by none.
    Shadow { claimed: u8 },
    Single { sensor: SensorId },
    Fused { seeing: u8 },
}

pub struct ShadowField {
    pub parameters: ShadowParameters,
    pub origin_enu: [f64; 2],
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<CellSight>,
    /// Each sensor's own sight, so PN-10 can show one sensor's shadow alone.
    pub per_sensor: Vec<(SensorId, Vec<bool>)>,
}

/// Areas, square metres, for the layer line, PN-11 and PN-16.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct ShadowMeasure {
    pub claimed_m2: f64,
    pub shadow_m2: f64,
    pub single_m2: f64,
    pub no_data_m2: f64,
}

/// The field for these sensors over this terrain at this height. Pure, like
/// `combined_coverage`, so the property tests in §11 drive it without a registry.
pub fn shadow_field(
    sensors: &[(SensorId, CoverageVolume)],
    terrain: &HeightField,
    parameters: ShadowParameters,
) -> ShadowField;
```

`CoverageVolume::covers` decides *claimed*; the line of sight decides *seen*. Nothing in
this note changes either type. `GapSeverity` is not reused for cells: a cell is not a run
along an approach, and DN-33's acceptance names runs.

## 6. The algorithm

### 6.1 One height field per load

The placed mesh is resampled once, when the terrain loads, at the DEM's own cell size
clamped to a cell budget (§9). A placed grid is still a grid, bent by its projection
(`ground.rs`'s own observation), so the resampling is `PlacedGround`'s search and
interpolation run once per raster cell, after which every lookup is an index and a
bilinear mix. The field is held beside the mesh in `DataStore` and the approach report
(`coverage_report`) switches to `HeightFieldLineOfSight` the same day, which is what closes
GAP-247.

**One consequence to say out loud.** The approach gaps will move when the lookup changes
from the nearest vertex to a bilinear surface, by up to one sample where a ray grazes a
ridge. DN-33 §5 makes any moved end a shape change, so **every standing acceptance
re-opens once** on the first report after the change, with `ShapeChanged` and what is
there now, journaled and audited against nobody, exactly as a mask loading does today.
That is the rule working, not a defect, and PN-11 lists it; the change should land in one
revision so it happens once.

### 6.2 The reference: sampled line of sight per cell

For every cell whose probe is claimed by at least one sensor, test `visible(sensor,
probe)` with `HeightFieldLineOfSight` for each claiming sensor. The probe is the cell
centre at the ground's up plus `height_agl_m` along the frame's up. This is the
definition, it is what the tests hold the production path to, and it is affordable only
for a small field: cells times claiming sensors times samples per ray. For a 40 km
square at 100 m cells, 160 000 cells, rays averaging 200 cells, three sensors: about 10
to the 8 bilinear lookups, a second or so in a release build on this desktop and well
inside MOP-14, so it is also a legitimate production path at coarse cells.

### 6.3 Production: a radial horizon sweep per sensor

For each sensor, cast one ray per azimuth at an angular step that keeps adjacent rays
within one cell at the field's far edge, and march each ray outward one cell at a time
keeping the **running maximum of the elevation angle to the terrain** seen so far. The
probe at a cell is seen when its own elevation angle from the sensor, with the height
above ground added, exceeds that running horizon. Each cell is visited about once per
sensor, so the sweep is linear in the number of cells: the 1.8 million cells of the same
square at 30 m take tens of milliseconds per sensor rather than the reference's minutes.
The sweep uses the same bilinear lookup, the same probe, and the same volume test as the
reference; what it changes is the order of the arithmetic.

The sweep is the production path and the reference is its oracle (§11 row b). Where the
two disagree it is on cells whose probe lies within one cell of a horizon line, which is
what "within one cell" in the criterion means.

### 6.4 Curvature, and what the rays do not model

The placed mesh already carries the Earth's curvature: each vertex's ENU up falls away
from the origin with distance, so a straight ray in the frame **is** the geometric line
of sight over a curved earth, and no 4/3-earth correction is applied on top. That is an
optical horizon. A radar sees slightly past it through standard refraction, so the field
reports **a little more shadow than a radar has**, which is the safe direction for a layer
whose failure mode is hiding a blind spot. A refraction factor is a stated non-goal
(§12); the parameters carry none, and the layer line says "geometric line of sight".

The probe height is added along the frame's up rather than the local vertical at the
cell. At 40 km the two lean 0.36 degree apart; for a 300 m probe that is 2 m, under any
cell size this note contemplates, and is bounded here rather than corrected.

### 6.5 When it is computed, and where

Not on the tick. A worker thread, started the way `terrain.rs` starts the loader, takes
a snapshot of the inputs, computes, and sends the field back; the tick polls it. It is
started again when any input changes: the set of searching and tracking sensors, a
sensor's position, sector or band, the terrain, the selected height, the cell size, or
the laydown option PN-16 has selected for a preview. The field carries a record of what it
was computed under, the sensor identities and modes, the terrain path, the baseline
revision, and the layer line says its age ("computed 6 s ago"), as WF-11's recompute line
always meant it to; "recomputing" while a newer one is on its way; and the budget is
MOP-14's. A field computed under inputs that have since changed is drawn, greyed, with
the line saying so, rather than torn down: a stale shadow is a better claim than none for
the seconds a recompute takes, provided it says it is stale.

## 7. Behaviour

1. **PN-11 gains the layer**: a toggle beside rings, gaps, hazards and geofences; the
   height chooser over the configured heights; the measure ("dead spots at 100 m AGL:
   12.3 km² of 310 km² claimed, 41 km² single-sensor, 0.8 km² no data"); its age; and
   a recompute control. The hidden-layer sentence on the viewport names it like any
   other, because a hidden shadow layer and a sector with no shadow look the same.
2. **The viewport draws the field under the rings** and over the terrain: the shadow is
   about the ground, the rings and gaps are drawn over it, and a track symbol is never
   under anything (GAP-007's order). The layer line says the height, the age, and
   "geometric line of sight".
3. **No terrain, no field, said in words** (§4 rule 4), with the same reason PN-09's
   terrain line gives: not configured, loading, or failed and why.
4. **The approach gaps use the height field's line of sight** from the same change
   (GAP-247), and the one-time re-opening of §6.1 is listed on PN-11 with its reason.
5. **PN-16 may ask for a field per selected option**, on the same worker, from the
   option's own placements through `laydown_coverage_volumes`; the options table gains a
   shadow-area column beside its coverage cell, computed on demand when an option is
   selected rather than for every row, because every row is a full recompute.
6. **PN-10 shows one sensor's own shadow** when a sensor row is selected, from
   `per_sensor`, which is how a sensor manager sees what re-aiming or raising that mast
   would do; DN-13's candidates may later score by shadow area (§12).
7. **The node computes no field** (D-148, §12) and `GET /v3/coverage` is unchanged. On a
   linked desktop the layer is the desktop's own, computed from the node's sensor
   snapshot and the desktop's DEM, and its line says so, as DN-33 amendment 1 already
   distinguishes the desktop's masked gaps from the node's flat answer.
8. **Nothing is journaled.** A field is derived from the baseline, the registry and the
   DEM, all of which are on the record; recomputing it from the journal gives the same
   answer, so it is not an event.

## 8. Rendering

### 8.1 The 2D projection, the verified path

The field is a raster, and a raster is drawn as **one textured quad**, not as a mesh of
cells: the field is turned into an `egui::ColorImage` once per recompute, uploaded as a
texture, and painted into the projected rectangle of the field's extent with
`Painter::image`. A 160 000-cell field is 640 KB of texture, uploaded once per recompute;
the per-frame cost is one quad, which is why this layer does not threaten the 8 ms egui
budget where a per-cell mesh would. `gungnir-viewport3d` already depends on `egui`, and
the texture handle lives in `ViewportState` beside the glyphs, created when the field
changes and never per frame (`rust-ui-architecture-coding-standards.md` §5).

The image carries the meaning in **pattern as well as colour**, as `draw_gaps_2d` does and
`../ux/accessibility.md` requires: shadow is a solid dark fill at the new palette token
`shadow_color` with its own alpha, single is a diagonal hatch in `warning_color`, no data
is a dot pattern in the muted text colour, and the hatch is baked into the image at a
period of a few cells so it reads at any zoom. Nothing is drawn for unclaimed or fused
cells, which keeps the terrain shading and the rings legible through the layer. The
tokens join `coverage_color` and `terrain_alpha` in `theme.rs`, in both palettes, under
the contrast test the other viewport colours already pass.

### 8.2 Over the three-d scene

`prepare_3d` draws every layer through the same `draw_layers` as the projection, as an
egui overlay over the GL output, so the field appears over the three-d scene **in the
same change**, identically, and the toggle cannot mean different things in the two
renderers. That satisfies the owner's "on a 3D map" for the scene as it stands today:
the scene is a straight-down perspective framing the same ground as the projection, and
a flat overlay on it is geometrically right. It is not a shadow draped on a terrain
surface seen at an angle; that needs the terrain in GL first, which is §13 increment 2,
and the reason it is not here is GAP-226: the GL draw has never been seen, and a
safety-relevant layer does not go onto an unverified renderer before the verified one.

## 9. Configuration and interface delta (D-147)

In `AnalyticsConfig`:

- `dead_spot_heights_agl_m: Vec<f64>`, the heights above ground the field may be
  computed at; each finite and positive, the list non-empty and strictly increasing,
  refused otherwise naming the entry. **Default `[100.0]`.** The first entry is the one
  shown at start.
- `dead_spot_cell_m: Option<f64>`, the raster cell size; finite and positive when
  stated. **Absent means the DEM's own cell size**, clamped so the field holds at most
  `DEAD_SPOT_CELL_BUDGET` cells (two million, about 45 km square at 30 m), with the
  clamp said on PN-11 when it bites.

Session state, not baseline, as `CoverageLayers` is: the toggle (on by default, for
DN-12 §7's reason) and the selected height.

No route, no event, no schema change; `SCHEMA_VERSION` stands.

**D-147** asked the owner for the default heights. The recommendation was one height,
100 m above ground, which is the band the threat table in
`../mission/air-defense-and-counter-uas.md` puts small UAS and terrain-following cruise
missiles in, with 30 m and 300 m as the entries a deployment adds when it wants the
low and the high case. A list of three by default would triple the first recompute for a
deployment that never looks at two of them. **Taken by the owner, 2026-10-29: the default
is `[100.0]`.**

## 10. User-interface delta

| Panel | Change |
|---|---|
| PN-11 Coverage layer controls | The dead-spot toggle, the height chooser, the measure, the age and the recompute control; the not-computed sentence; the acceptances re-opened by GAP-247's one-time change, listed with their reason |
| PN-02 Viewport | The field as a textured layer between the terrain and the rings; the layer line with height, age and "geometric line of sight"; the hidden-layer sentence names it |
| PN-01 Status strip | No change. The strip already says when terrain masking is unavailable, and a second coverage figure on it is more than the strip should carry |
| PN-16 Planning | A shadow-area column per option, filled on selection |
| PN-10 Sensor management | The selected sensor's own shadow on the map |
| PN-09 System health | The terrain line gains the height field's cell size and cell count, and the field's worker state |

## 11. Verification

Rows for `../verification-capability-table.md` §2, as first drafted; agreed on 2026-10-29
with amendment 1 item 8's two restatements, and gates since (amendment 2):

| Component | Test | Method | Criterion |
|---|---|---|---|
| `gungnir-analytics` shadow geometry (CAP-2.11) | `gungnir-analytics/tests/shadow_geometry.rs` | A wall of height *H* at distance *d* from a sensor at height *h<sub>s</sub>* on a flat field; probes at a height *h<sub>p</sub>* | For *h<sub>p</sub>* above the wall, the shadow ends at *x* = *d*·(*h<sub>p</sub>* − *h<sub>s</sub>*)/(*H* − *h<sub>s</sub>*) from the sensor, recovered within one cell; for *h<sub>p</sub>* at or below the wall, every cell beyond it is shadow; a cone and a valley fixture likewise, each with its closed-form horizon |
| `gungnir-analytics` sweep against reference (CAP-2.11) | the same file | Random synthetic terrain (sums of ridges), several sensors, several heights; the sweep against §6.2's sampled reference | Every disagreeing cell lies within one cell of a cell where the reference's own answer changes; the disagreeing fraction under 1 percent |
| `gungnir-analytics` monotonicity (CAP-1.4) | the same file | Property tests, as DN-12 §8's | Raising the probe height never adds a shadow cell; removing a sensor never removes one; a standby sensor changes nothing; the field carries its height and cell size |
| `gungnir-analytics` agreement with the approach report (CAP-1.4) | the same file | An approach sampled at the field's height over the same terrain and sensors | Every approach sample's severity agrees with the cell under it, except samples within one cell of a cell boundary |
| `gungnir-app`, `gungnir-viewport3d` the layer (CAP-5.9) | `gungnir-app/tests/dead_spots.rs` over the render probe | Round 1's baseline with a DEM holding one ridge; no DEM; the layer hidden; a sensor dropped to standby | The dark cells are where the ridge is and nowhere else; with no DEM the not-computed sentence reaches the screen and nothing dark does; hidden is named in the hidden-layer sentence; the field is recomputed off the tick and its age is drawn; the standby sensor's shadow leaves with its ring |
| `gungnir-app` the recompute budget (MOP-14) | `gungnir-app/tests/frame_budgets.rs`, release only | A 1 000 by 1 000 synthetic DEM, three sensors, one height | Under 10 s from the change to the field; no frame of the tick waits on it |

## 12. What this note deliberately does not do, and D-148

- **No refraction factor.** §6.4: geometric line of sight, conservative for radar; a
  per-sensor *k* is a later parameter on the volume, not on this field.
- **No probability of detection.** A cell is seen or not by geometry. Range, clutter,
  radar cross-section and the rehearsal's detection model (`SensorParams`) are a
  different question, and folding them in would make the dark area mean "unlikely" rather
  than "impossible", which is a different layer.
- **No buildings or vegetation.** A bare-earth DEM understates shadow in a town or a
  forest. The field is honest about its input by naming the DEM; a surface model is the
  same loader with a different file, and is not this note's to decide.
- **No moving sensors, and no time.** A sensor's position is the registry's.
- **No shadow volumes.** The field is one height at a time; stacking heights into a
  volume is §13 increment 4.
- **No node computation (D-148).** The node loads no terrain (D-130) and answers coverage
  flat. Computing the field on the node means the node loading a DEM, a baseline change
  on every node and a second place the height field has to agree, for a layer only a
  desktop draws. Recommended desktop only, the layer saying so when linked, as
  DN-33 amendment 1's masked-versus-flat distinction already does; the node's turn comes
  if and when a headless consumer, DN-13's recommender on the node, wants shadow area.
  **Taken by the owner, 2026-10-29: desktop only.**
- **No scoring of re-tasking candidates by shadow area.** Named for DN-13 as a follow-on;
  it is a change to a scorer with its own verification row.

## 13. Other 3D elements, as future work

The owner asked for this to be analysed as a later task. What follows is the inventory
of what a 3D picture would add, what the code already holds for each, what each needs,
and the order the dependencies between them impose. **The one fact that orders
everything is GAP-226**: the three-d scene is the default renderer and its draw has never
been seen on a display, because there is no headless probe for a GL draw the way
`RenderProbe` reads egui's shapes. Every item below puts more on that path, and none of
them should go there before the path has been looked at.

### 13.1 The inventory

| # | Element | What exists | What it needs | Risk |
|---|---|---|---|---|
| 0 | **Seeing the scene** (GAP-226) | The attachment, instanced glyphs, a camera coupled to the 2D view, the runtime toggle | A screenshot smoke check on a real display, `scene_3d` defaulting off until it passes, the two contradicting comments corrected | None to the picture; it is the gate for every row below |
| 1 | **Terrain surface in GL** | `TerrainMesh` with grid shape, placed, decimated for the 2D draw; `three-d` draws a `Gm<Mesh, ColorMaterial>` with per-vertex colours | One GL mesh built per load, never per frame (standards §5), from the same stride decimation; the height colour ramp `terrain_color` reused; the 2D shaded surface stays the fallback | Depth: a terrain mesh under flat overlays makes the egui overlay (rings, gaps, this note's field) float at ground level zero while the ground rises; so the overlay layers move into GL at the same time or the scene stays straight-down |
| 2 | **Draped layers** | §8's `ColorImage` of the field; the rings and gaps as ENU geometry | The same image as a `three_d::Texture2D` on the terrain mesh's UVs; rings and gaps as GL line geometry on the surface (`update_intercept_geometry`'s `NotImplemented` is the same shape of work) | The one-`draw_layers` rule: a layer hidden in one renderer and drawn in the other would make the toggle lie; the GL drape takes the same `LayerInputs` and the same `CoverageLayers` |
| 3 | **A camera that tilts and orbits** | `TopDownView` drives both renderers; `gl::camera` derives height from the zoom so the two show the same ground | Pitch and yaw in session state, applied only to the GL camera; the 2D view keeps centre and zoom, and the status line says the scene is tilted, so "the same ground" stays true for the centre and the scale | Two cameras for one picture under one label was rejected in `gl.rs`; a tilt is the minimum that keeps that |
| 4 | **Volumes** | `CoverageVolume` with range, sector and band; the field at one height | Coverage volumes as translucent shells (a sector of a spherical band), drawn back to front; shadow at several heights as stacked columns | Translucent shells over each other read badly past three sensors; an operator chooses one, as PN-10's single-sensor view |
| 5 | **Tracks with altitude** | Glyph instances already at the track's ENU up; sigma on the glyph | A drop line to the ground under each glyph, an uncertainty ellipsoid at two sigma, bearing rays as 3D wedges, predicted paths with altitude, the dense group's discs at altitude; labels stay in the egui overlay, where text is cheap and legible | Occlusion: a glyph behind a hill must still be visible, so glyphs are drawn without depth test against the terrain, which is also what keeps "a track symbol is never under anything" true in 3D |
| 6 | **Rules and things on the ground** | Geofences as circles, hazards as outlines, in 2D; defended assets and effectors are **not drawn at all, in either renderer** (review §8 finding 3) | Assets and effectors drawn in 2D first, then as markers in GL; geofences as prisms need a floor and a ceiling the model does not carry (a model delta); engagement envelopes as domes need an altitude band on DN-04's effector (a model delta) | The model deltas are design, not drawing, and belong in amendments to DN-01, DN-04 and the geofence section before any GL work |
| 7 | **Point clouds in GL** | `PointBuffer` loaded (GAP-098); drawn as dots in 2D, **not placed** against the deployment's origin | A `three_d::PointCloud` built per load; placement through the same CRS path the DEM takes (GAP-098's own note names this as missing) | Drawing an unplaced cloud in 3D claims an alignment nothing computed, the same honesty problem the 2D note records |
| 8 | **Basemap and imagery** (GAP-227, D-139) | Nothing: a metric grid; the streaming module is three `NotImplemented` stubs; no 3D Tiles dependency is admitted | A signed-off tile source and dependency (§2.9), the screen-space-error traversal, imagery as a drape on the terrain, MIL-STD-2525D symbols per D-139 | The largest item and the one with the most decisions; it is last by dependency, not by value |
| 9 | **Picking in 3D** | Selection originates only in the track table (review finding 3); the viewport takes drag and scroll | A ray from the cursor through `gl::camera` against the glyph instances, as a pure function with tests, and selection as a `PanelAction` like the table's | Picking in 2D is the same function on a simpler camera and should land first, since the table is today the only way to hook a track |

### 13.2 The order

0. GAP-226 (see the scene, or default it off).
1. Terrain in GL with the overlay layers moved onto it (rows 1 and 2 together), still
   straight down, so what the scene shows is what the projection shows, draped.
2. The tilt (row 3), now that there is a surface to tilt over.
3. Tracks with altitude (row 5), then the ground elements (row 6) after their model
   deltas.
4. Volumes (row 4) and point clouds (row 7).
5. Picking (row 9), in 2D first.
6. Basemap, imagery and symbology (row 8), under D-139 and a dependency decision.

### 13.3 The rules every row keeps

- No GL object is created per frame, and everything that decides *what* is drawn stays a
  pure function with tests, which is how `gl.rs` is written and the only way this crate
  can be tested without a display.
- One `draw_layers`, one `LayerInputs`, one set of toggles for both renderers; the scene
  makes no claim the projection does not.
- A layer that cannot be computed says so in words, in the scene as in the projection.
- No new dependency edge: `gungnir-viewport3d` already reaches `gungnir-data`,
  `gungnir-data-fusion`, `gungnir-model` and `gungnir-ui`, which is everything the rows
  above read; a 3D Tiles crate is a §2.9 decision, not an edge.

## Amendment 1, 2026-10-29: what the build changed

GAP-247 and GAP-246 were built the day this note was written. Where the build departed
from the text above, the code is right and this says why; §1 to §13 stand otherwise.

1. **Rays sample every half cell, not every cell** (§5, §6.2, §6.3). At a whole cell a
   crest between two samples is stepped over, and two rays a few metres apart disagreed
   by the height of a ridge. `HeightFieldLineOfSight` and the sweep both sample at half
   the terrain cell.
2. **The height field's cells sit on the DEM's vertices** (§6.1). The first draft's raster
   started at the vertices' corner, half a cell off them, which smoothed every edge a
   second time; the wall test found its shadow three cells late. Centred on the vertices,
   resampling at the DEM's own cell copies the surface.
3. **The sweep knows its own error and hands the doubtful cells to the reference** (§6.3).
   The ray that decides a cell runs beside the true line to its centre, and over steep
   ground that offset reads different ground: one test cell was hidden to the sweep and
   seen by the reference with ten metres to spare. Each verdict now carries a bound, the
   steepest slope in the field times the ray's offset plus half a step, over the cell's
   distance; a cell inside it is decided by the reference. Over two 360 000-cell test
   fields the two then disagree on no cell at all, and the sweep stays four times
   (steep) to twelve times (rolling) faster than the reference in a release build.
4. **The types grew where the build needed them** (§5). `per_sensor` carries a
   `Sight` (unclaimed, no data, hidden, seen) rather than a bool, since one sensor's
   shadow needs its claim as well as its sight; the field carries `clamped_from_m` for
   PN-11's sentence; `measure_cells` measures one sensor's cells; `shadow_field_reference`
   is public, the oracle. `PlacedGround`'s lookup moved down as `MeshSurface`, with the
   wording of its refusals kept.
5. **One sensor's shadow is chosen on PN-11, not PN-10** (§7 item 6, §10). PN-10 has no row
   selection to hang it on; PN-11 has the height chooser already, and the chooser beside
   it names the sensor, as does the map's line.
6. **The approach report is cached by its inputs** (§6.1, GAP-247's action): the volumes,
   the placed approaches, the parameters and the surface by identity. The one-time
   re-opening of §6.1 is not singled out on PN-11, because the desktop cannot tell a
   shape change the lookup caused from any other; each is listed as a shape change.
7. **The map's cells are folded on the worker** (§6.5). Folding a million-cell field into
   the viewport's cells on the tick that received it cost one 7 ms tick; on the worker
   the slowest tick while a field is computed is 105 µs.
8. **Two draft criteria were restated by their tests** (§11). The wall's top begins at
   the first DEM cell centre on it, since the surface runs through the centres, and the
   tolerance is a cell plus the ramp's half cell carried along the line. "Removing a
   sensor never removes a shadow cell" is false where only that sensor claimed the cell,
   which becomes unclaimed; the property tested is that removing a sensor never turns a
   dead spot into a watched cell, and that what only it saw stops being watched. The rows
   stay drafts here: a pass criterion is human-owned.
9. **Measured** (`gungnir-app/tests/frame_budgets.rs`, release, not asserted): over a
   1 000 by 1 000 DEM at 30 m with three radars of 12 km, the field of 1 301 by 1 267
   cells is on the map 363 ms after the change, against MOP-14's 10 s.
10. **PN-09** says the resampled surface's cell and size and the worker's state on the
    terrain's own line, not a line of its own.

## Amendment 2, 2026-10-29: the rows agreed, and the line of sight made exact

1. **§11's six rows are agreed and are gates**, in `../verification-capability-table.md`
   §2 ("Rows added by DN-34"), as amendment 1 item 8 restated two of them. What the owner
   has signed is in [`../signatures.md`](../signatures.md).
2. **The first row's cone and valley were not built with the wall, and are now**
   (`gungnir-analytics/tests/shadow_geometry.rs`): a cone's shadow begins at the line over
   its apex, and a valley's runs from the line over its rim to where the far slope rises
   into sight, each within one cell of the closed form, through both paths.
3. **The line of sight is exact, not sampled** (amending amendment 1 item 1). The cone
   showed that a ray stepping every half cell can read a sharp peak several metres low,
   which hides nothing it should and shows ground it should not: the unsafe direction.
   The interpolated surface bends only where a ray crosses a line through cell centres,
   and between two crossings its height along the ray is a parabola, so
   `HeightFieldLineOfSight` checks every crossing, both ends and each piece's own top.
   The sweep reads the same crossings into its horizon as well as its half-cell steps.
4. **The recompute is 645 ms**, not amendment 1 item 9's 363 ms, on the same machine and
   fixture: the exact test reads more points a ray. MOP-14's budget is 10 s, and the test
   now asserts it in the release profile, with the structural half of the row, that the
   tick which sees a change returns with the old field still drawn.
5. **A test fixture's own mistake, recorded because it will recur.** A field's cells are
   laid out from its sensors' reach, so a test reading the cells "along" a line must put a
   row of cell centres on that line, or it reads probes half a cell to one side. The wall
   and the valley do not vary across the line and could not show it; the cone could.

## Amendment 3, 2026-10-29: a laydown option's dead spots (GAP-248)

§7 item 5 is built as it reads. Selecting an option on PN-16 computes its field on the
dead-spot worker, from the sensors it places that are searching or tracking, where it
places them, through the same `laydown_coverage_volumes` its coverage column reads, at the
height and cell the live field uses. The options table gains a "Dead spots" column, its
meaning on the header's hover text. The selected row reads its hidden area within the area
its sensors claim, at its height, and "recomputing" while its inputs change. The extra
column pushed the table's last column past the edge of a 1 400 px pane, where a cell is not
drawn at all, so the table now scrolls sideways rather than cutting a column off unseen. Every other row
reads "select to compute", and a row with no DEM or no frame says why. One option is
computed at a time and the field is dropped when the selection is cleared, so nothing is
computed for options nobody is looking at.

Two things it does not do. It draws nothing on the map: the map's layer stays the live
field, and an option's preview there is its placements (GAP-087), so the map never shows
two fields at once. And it shows no difference from the current laydown's dead spots: that
needs the current option's field computed too, which item 5 left out for the same reason it
computes on selection.

## Traceability

GAP-246, GAP-247; D-146, D-147, D-148; CAP-1.4, CAP-2.10, CAP-2.11, CAP-5.9; MT-07
step 3, MT-09 step 2; MOP-14; `../mission/operational-environment.md` §2;
`../ux/task-analysis/sensor-manager.md` 1.2, `planner.md` 2.2; WF-02, WF-11; PN-02,
PN-09, PN-10, PN-11, PN-16; `../review/2026-10-26-comprehensive-review.md` §8 findings 3,
5 and 6; GAP-226, GAP-227, D-139. Related notes: DN-12 (the volumes and the approach
report this extends), DN-33 (the acceptances §6.1 re-opens once), DN-13 (the candidates
that may later score by shadow), DN-26 and DN-32 (laydown options and the rehearsal's own
flat horizon), DN-01 and DN-04 (the model deltas §13 row 6 names).
