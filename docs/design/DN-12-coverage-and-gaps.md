# DN-12 Combined coverage and gap detection

Closes GAP-006. Status: first draft, 2026-09-05. **Design only; no code exists.** What the
owner has signed of this note is in [`../signatures.md`](../signatures.md).

## 1. The gap and the thread step it blocks

`CoverageVolume` computes whether one sensor covers one point. Nothing combines volumes
across the registry and reports where the picture has holes. MT-09 laydown and MT-07
degradation therefore rely on the operator reading coverage circles by eye, which is
exactly the task a person is worst at and a computer is best at.

## 2. The owning component

`gungnir-analytics`, which already owns `CoverageVolume`, `LineOfSight`, and `viewshed`.

## 3. Types

In `gungnir-analytics`:

```rust
/// Coverage of one point by the registry as a whole.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointCoverage {
    pub position_enu: [f64; 3],
    /// Sensors that cover it, after range, elevation, and terrain masking.
    pub sensors: Vec<SensorId>,
    /// Highest confidence among the covering sensors; zero when none.
    pub confidence: f32,
}

impl PointCoverage {
    /// Covered by at least two sensors, which is what makes a track fusible
    /// rather than merely detectable.
    pub fn is_redundant(&self) -> bool { self.sensors.len() >= 2 }
}

/// A hole along an approach: a contiguous run of uncovered samples.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageGap {
    /// The approach this gap lies on, by index into the input.
    pub approach: usize,
    pub from_m: f64,
    pub to_m: f64,
    /// Sample positions, so the viewport draws the gap and not a guess at it.
    pub samples: Vec<[f64; 3]>,
    pub severity: GapSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
         serde::Serialize, serde::Deserialize)]
pub enum GapSeverity {
    /// Covered by one sensor only: detectable, not fusible.
    SingleSensor,
    /// Covered by nothing.
    Uncovered,
}

/// Coverage of a set of approaches by a set of sensors at a moment.
pub fn combined_coverage(
    sensors: &[(SensorId, CoverageVolume)],
    los: &dyn LineOfSight,
    approaches: &[&[[f64; 3]]],
    sample_spacing_m: f64,
) -> Vec<CoverageGap>;
```

The function takes sensors as pairs rather than reading a registry, which is what keeps it
a pure function that a property test can drive.

## 4. Edges

**One: `gungnir-analytics` to `gungnir-sensor-management`.**

`combined_coverage` itself needs no edge. What needs one is the convenience that builds its
input from the live registry:

```rust
pub fn coverage_from_registry(registry: &dyn SensorRegistry) -> Vec<(SensorId, CoverageVolume)>;
```

The edge is taken under the owner's rule of 2026-09-05 rather than making every caller
rebuild the mapping from `SensorRecord` to `CoverageVolume`, which is where the same three
lines would be duplicated in the app, the node, and the decision recommender.

**Reviewer note carried forward:** `gungnir-analytics` currently depends on
`gungnir-coord`, `gungnir-data`, and `gungnir-geo`, all of which are below it. Adding
`gungnir-sensor-management` makes it depend on a configuration-reading crate too, which
makes analytics more central than it was. It is acyclic and it is legitimate, and the
reviewer may still prefer the conversion to live in the binaries. If so, only
`coverage_from_registry` moves and nothing else in this note changes, which is why the
pure function is defined first.

Recorded in [`dependency-edges.md`](dependency-edges.md).

## 5. Behaviour

Sample each approach polyline at the given spacing, evaluate every sensor's volume and
line of sight at each sample, and group contiguous runs of samples whose coverage is below
the threshold into gaps.

Rules:

1. **Only sensors that are actually searching or tracking count.** `SensorRegistry::coverage`
   already applies this, and the registry-derived input inherits it. A sensor at standby
   contributes nothing, which is what makes the coverage view useful during MT-07.
2. **Single-sensor coverage is reported as a gap of its own severity**, not as coverage.
   One sensor gives a bearing and a range; it does not give a fusible track, and a laydown
   that looks covered but is single-sensor everywhere is a laydown that fails on the first
   loss.
3. **Terrain masking is applied when a terrain model is available, and its absence is
   reported.** `FlatTerrainLineOfSight` is optimistic by construction. A coverage answer
   computed on flat terrain is labelled as such, because the difference in a valley is the
   whole answer.
4. Sample spacing is an input and appears on the output, so a coarse run cannot be mistaken
   for a fine one.

**Where approaches come from.** The caller supplies them. For MT-09 they are the planner's
threat axes; for MT-07 they are the axes toward the defended assets from DN-01. This note
does not invent an approach model, which would be a mission-analysis question dressed as a
design one.

## 6. Configuration and interface delta

`ConfigBaseline.analytics.coverage_sample_spacing_m: f64`, defaulted and validated as
finite and positive.

Interface: `GET /v2/coverage` for the configured approaches, authorization action
`picture.view`. New endpoint, additive.

**Correction, 2026-09-05 (GAP-006).** This section wrote the
response as a bare `Vec<CoverageGap>`. §5 of this note puts the sample spacing and whether
terrain masking was applied **on the result**, so that a coarse run cannot be mistaken for
a fine one -- and a response carrying only the gaps throws away exactly what that rule
preserves. The route returns the whole `CoverageReport`.

A second addition for the same reason: a node that has computed nothing returns
`CoverageResponse::NotComputed` with the reason, rather than an empty gap list. A
deployment with no declared local frame origin cannot place geodetic positions in a common
picture at all, and one with no approaches has nothing to measure along; in both cases an
empty list would read as a clean sector, which is the confusion §7 exists to prevent.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-11 Coverage layer controls | The panel's reason for existing. Coverage and gaps as toggleable layers, with a before-and-after comparison for DN-13 |
| PN-02 Viewport | Gaps drawn along approaches, single-sensor distinguished from uncovered by pattern as well as colour |
| PN-16 Planning panel | Coverage per laydown option, which is how options are compared |
| PN-01 Status strip | A count of uncovered approach segments, and a note when terrain masking is unavailable |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-1.4 Coverage and gaps | Property tests over generated sensor sets and approaches, plus an MT-07 replay | A point inside exactly one volume yields `SingleSensor`, not covered; removing a sensor never shrinks the reported gap set; standby sensors contribute nothing; the output carries the sample spacing and whether terrain masking was applied | Generated sensor layouts; TT-07 sample set for the degradation case |

The monotonicity property in the second clause is the one that catches most implementation
errors in a coverage routine.

## Traceability

GAP-006; CAP-1.4; MT-07 step 3, MT-09; depends on GAP-003 for the wired registry and DN-01
for asset positions; read by DN-13; `../ux/wireframes/WF-11-coverage-layers.puml`,
`WF-16-planning.puml`; principle AP-02.

## 9. Amendment 1: a coverage volume has a bearing

Raised 2026-09-25 by GAP-118, under D-84.

`CoverageVolume` had a range and a lower elevation limit and no azimuth, so a panel radar,
a fixed camera or a sensor masked on one side by its own mast was counted, drawn and
compared as covering the full circle, and the Coverage accuracy row's bearing clause had
nothing to check.

1. **`gungnir_model::AzimuthSector`**: a boresight and a width in `(0, 2π]`, bearings
   clockwise from north, contained across the wrap through north. It lives in the model
   because the baseline, the registry, the analytics and the viewport all carry it (AP-06).
2. **In a baseline a sector is against true north at the sensor**, which is how a survey
   states one: `SensorConfig.azimuth_sector`, and a laydown's
   `SensorPlacement.azimuth_sector` for a placement that re-aims its sensor. **Absent is
   the full circle**, on both, so every baseline written before sectors existed means what
   it meant; a placement without its own takes the declaration's. Validation refuses a
   non-finite boresight or a width outside `(0, 2π]`, naming the sensor or the placement,
   and `analytics.coverage_min_elevation_rad` is now refused outside `[-π/2, π/2]`.
3. **A `CoverageVolume`'s sector is in the local frame**, because `covers` is.
   `LocalFrame::true_north_at` gives the frame bearing of true north at a position -- the
   meridian convergence, about 0.1 degree eleven kilometres east of an origin at 45 degrees
   north and 0.35 degree at 39 km -- and `LocalFrame::sector_in_frame` turns a sector with
   it. `coverage_from_registry` now takes the frame rather than a conversion closure so it
   can do this, and `volume_of` gives one sensor's volume for DN-13's candidates.
4. **PN-11 draws a sector as a wedge** from the sensor to its range between its edges; the
   full circle stays a ring. PN-16 and DN-13's recommendations count coverage only inside
   the sector.

**Found by the fixture**: the approach sampler restarted its spacing at every vertex, so a
segment shorter than the spacing contributed no sample and a curved or digitised approach
was judged by its first point. It now spaces samples along the whole polyline.

Not in this amendment, and filed as GAP-158: an upper elevation limit, a lower one per
sensor rather than one for the baseline, and elevation measured against the sensor's own
vertical rather than the frame's. The verification is
`gungnir-analytics/tests/coverage_accuracy.rs`; the row itself is unchanged, and walking
it is the owner's.

## 10. Amendment 2: a sensor's own elevation band, against its own vertical

Raised 2026-09-26 by GAP-158, under D-111.

Amendment 1 left every sensor credited from the baseline's one
`analytics.coverage_min_elevation_rad` up to the zenith, measured against the frame's
vertical: a radar with a 2 degree mask and a camera looking up at 30 degrees had the same
floor, nothing had a ceiling, so a radar's cone of silence overhead was counted as
covered, and a sensor 11 km or more from the origin had its limit off by more than the
Coverage accuracy row's 0.1 degree.

1. **`gungnir_model::ElevationBand`**: a floor and a ceiling, radians above the horizon at
   the sensor, both in `[-π/2, π/2]` with the ceiling above the floor. A ceiling of `π/2`
   is the zenith, which is no ceiling. **A band is stated whole**: a floor and a ceiling
   are one survey of one sensor, and composing a floor from one place with a ceiling from
   another could make an empty band nobody stated.
2. **Declared on the sensor, optionally on a laydown placement**, as amendment 1 did the
   sector: `SensorConfig.elevation_band` and `SensorPlacement.elevation_band`, both absent
   by default. **The precedence, highest first: the placement's band, the declaration's,
   then the baseline's `coverage_min_elevation_rad` as the floor with the zenith as the
   ceiling** (`gungnir_analytics::band_or_default`). The last is exactly what every sensor
   had before this amendment, so every baseline written before it means what it meant.
   Validation refuses a band with a non-finite limit, a limit outside `[-π/2, π/2]`, or a
   ceiling not above its floor, naming the sensor, or the laydown and the placement.
3. **Measured against the sensor's own vertical.** `LocalFrame::vertical_at` gives the
   ellipsoid normal at a position as a unit vector in the frame -- the frame's `u` at the
   origin, leaning about 0.009 degree per kilometre away from it -- taken numerically from
   the frame's own conversion as `true_north_at` is. `CoverageVolume` gains
   `max_elevation_rad` and `vertical` (serde defaults `π/2` and `[0, 0, 1]`, so a volume
   serialized before them reads as it did), and `covers` measures elevation against
   `vertical` and bearing in the plane square to it. A vertical that is no direction covers
   nothing but the sensor's own position: the direction that shows a gap rather than
   hides one.
4. **One place builds a volume from a declaration**, `gungnir_analytics::volume_in_frame`:
   position, sector turned by the convergence, band against the vertical, all where the
   sensor stands. `volume_of` (the registry's path and DN-13's candidates) and PN-16's
   `laydown_coverage_volumes` both call it, so PN-11's gap report, PN-16, DN-13's
   recommendations and the node's `/v2|v3/coverage` answer place a sensor identically.
   PN-11's rings still draw the horizontal footprint to the nominal range; the band
   changes which approach samples are counted, which is what the gap layer draws.
5. **A laydown rehearsal honours it too** (DN-32 §13): on top of the detection model's own
   altitude band, before any draw.

The verification is `gungnir-analytics/tests/coverage_accuracy.rs`: the frame-stated
fixture now states a ceiling on five of its six volumes and recovers floor and ceiling;
the geodetic fixture's floor is recovered against each sensor's own vertical; and a third
test declares bands on sensors up to 47 km from the origin, runs them through the
registry, recovers every floor and ceiling in a frame anchored at the sensor within the
criterion (worst 0.055 degree), and shows that the same volume measured against the
frame's vertical misses it by 0.37 degree at the floor and 0.40 at the ceiling. PN-16's
path is `gungnir-app/tests/planning_panel.rs`'s
`a_laydown_counts_coverage_only_inside_the_sensor_s_elevation_band`, and the node's
coverage answer `gungnir-node`'s
`the_coverage_answer_credits_a_sensor_with_its_declared_elevation_band`. The row itself is
unchanged, and walking it is the owner's.
