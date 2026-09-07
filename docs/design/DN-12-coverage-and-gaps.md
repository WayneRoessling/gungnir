# DN-12 Combined coverage and gap detection

Closes GAP-006. Status: first draft, 2026-09-05. **Design only; no code exists.**

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

**Correction, 2026-09-05 (GAP-006), signed by the owner 2026-09-05.** This section wrote the
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
